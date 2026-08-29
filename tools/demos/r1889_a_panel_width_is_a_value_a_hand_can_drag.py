#!/usr/bin/env python3
"""R1889 §5.32 §5.38 §2 #7 — **a side panel's width is a value a hand can drag,
and every derivation follows it.**

# What this demo exists for

The campaign `the arrangeable unit is a panel and should be an area` reached
this round with three exit conditions left, and the first is the reference
editor's third region operator: **`region_scale`**. R1887 gave a panel its edge
(`region_flip`) and its fold (`region_toggle`) and left the size, which was
measured at R1889 as the field of the placement value that **nothing in the tree
could write** — zero writers of `extent` outside the two constants that seed it.

So a reader could move a panel and could not resize it, which is the half of the
original report (*why can these panels not be moved?*, asked three times) that
survived R1887.

# Two hands, and which is which

The DECLARATION is what bounds each pane promises, in the specification's own
words. The BEHAVIOUR is what the running screen does when a hand or a caller
pushes on it. Both are read from the **assembled** application —
`hello-analyzer-shell`, at the seat where that screen is mounted — because the
panels belong to a tool a person opens, and a claim about the guest alone is a
claim about a binary nobody runs. The two hands are two SOURCES, which is what
made them worth having; they were never two processes.

★★★★★ **R1890 repaired this section, and the repair was a re-measurement.**
R1889 drove the declaration and the wire refusal in a SECOND process, on the
finding that the guest's introspection surface does not survive mounting:
`graph`, `zoom`, `running`, `verdict`, `nodes` and `links` all answered in a
standalone lab and all six answered `UnknownIntrospectPath` through the shell.
Re-measured at R1890 against the same build, every one of them answers — at
`/<screen tag>/external/<path>`. R1889 had asked at `/external/<path>`, the ROOT
short-circuit, which in an assembled application is the HOST's surface, so those
six refusals were true statements about the shell rather than about the guest.

The address is now published by the roster (`destinations[].screen.address`) and
read from there below, so this demo asks the application where a screen answers
instead of assuming a grammar. `tools/demos/r1890_a_mounted_screen_answers_on_
the_wire.py` is where that property is held; here it is simply used.

What this drives:

* **A** — in the assembled tool: a grip is painted exactly where the
  specification says a pane resizes, and nowhere else.
* **B** — ★ a real pointer drag arc on that grip widens the panel, and the four
  derivations follow: the pane, the canvas beside it, the toolbar above it, and
  the pane's own BODY — the last being the one this round had to move, since ten
  rectangles inside the inspector stated their width from the opening constant.
* **C** — the accessibility tree publishes the width as a value WITH its bounds,
  which is the whole of what a reader who never sees the drawing needs.
* **D** — over the wire, a width outside the declared range is REFUSED and the
  refusal carries the range. The floor accepts an out-of-policy placement in
  silence (measured R1801).
* **E** — and a drag past the bound CLAMPS where that wire call is refused. One
  declaration, two readings.

# Floor

The floor toolkit resizes a docked panel, so this axis is not one where it lacks
the gesture — the row that matters is what comes with it. Measured offscreen at
R1801 against a built 6.11.1: that toolkit's arrangement round-trips as opaque
bytes no reader can diff, it has no fold at all, and a placement its own
declaration forbids is accepted silently. Here the placement is a readable
value, every refusal names what was asked and what was allowed, and the bounds
reach a screen reader as a range rather than as prose.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell -p hello-node-lab
    DISPLAY=:97 python3 tools/demos/r1889_a_panel_width_is_a_value_a_hand_can_drag.py
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
    run_demo,
)

SHELL = "hello-analyzer-shell"
#: The HOST's own root surface. R1890 — naming it `HOST` rather than `EXT`
#: because that is what it is, and reading it as "the external" is precisely the
#: mistake that sent R1889 looking for the guest's paths here.
EXT = "/external"
SEAT = "lab"
#: The pane this demo drives. The inspector, because it is the one whose body
#: rectangles read the opening constant until this round.
PANE = "lab.inspector"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rects(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint"))


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def js(value):
    """A published value, whether the surface handed back JSON or a string."""
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    """Where the screen mounted at `seat` answers, as the application says.

    R1890 — asked rather than composed. The roster publishes each mounted
    destination's address, so a caller never has to know that a surface lives at
    `/<tag>/external/…`; R1889 did not know it, asked at the root, and concluded
    from seven true refusals about the host that the guest was unreachable.
    """
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def declaration(app: RpcSubprocess, surface: str) -> dict:
    """Each pane's row of the specification, in the screen's own words.

    The second hand. A demo that took the bounds from this file would agree
    with itself after somebody changed the declaration; taking them from what
    the screen happens to draw would agree with whatever it happened to do. So
    they come from the specification the screen PUBLISHES, and the drawing is
    then held to them — two sources, one process.
    """
    said = js(app.query(f"{surface}/spec"))
    return {pane["tag"]: pane for pane in said["panes"]}


def section_a(app: RpcSubprocess, declared: dict) -> None:
    banner("A — a grip is painted exactly where a pane declares it resizes")
    painted = rects(app)

    resizes = sorted(t for t, p in declared.items() if p.get("resize") is not None)
    ok(
        "A: the specification names at least one resizable pane, so nothing "
        "below runs zero times",
        len(resizes) > 0,
    )
    print(f"  [population] {len(declared)} pane(s), {len(resizes)} declare a resize")

    for tag, pane in sorted(declared.items()):
        grip = f"{tag}.grip"
        if pane.get("resize") is None:
            ok(
                f"A: `{tag}` declares a fixed width and paints no grip — an "
                f"affordance that cannot act is one that lies",
                grip not in painted,
            )
        else:
            ok(
                f"A: `{tag}` declares {pane['resize']} and paints `{grip}` in "
                f"the ASSEMBLED tool",
                grip in painted,
            )


def section_b(app: RpcSubprocess, declared: dict) -> None:
    banner("B — ★ a real drag arc widens the panel, and four derivations follow")
    pane = declared[PANE]
    bounds = pane["resize"]
    body_tag = pane["body"]

    before = rects(app)
    pane0, canvas0, toolbar0 = before[PANE], before["lab.canvas"], before["lab.toolbar"]
    body0 = before[body_tag]
    gx, gy, gw, gh = before[f"{PANE}.grip"]
    start = (gx + gw // 2, gy + gh // 2)

    # The inspector sits on the right, so dragging LEFT widens it. Read off the
    # published placement rather than assumed, so this stays right if the
    # opening arrangement is ever flipped.
    edge = pane["at"]["edge"]
    widen_by = 60
    end = (start[0] + widen_by, start[1]) if edge == "left" else (start[0] - widen_by, start[1])

    app.drag(from_at=start, to_at=end, steps=8)
    app.tick_ms(16)

    after = rects(app)
    pane1, canvas1, toolbar1 = after[PANE], after["lab.canvas"], after["lab.toolbar"]
    body1 = after[body_tag]

    ok(
        f"B: ★★★★★ the panel is wider — {pane0[2]} -> {pane1[2]} logical pixels, "
        f"dragged through the HOST with a pointer arc",
        pane1[2] > pane0[2],
    )
    assert_eq(
        canvas0[2] - canvas1[2],
        pane1[2] - pane0[2],
        "B: ★★ the canvas gave up EXACTLY what the panel took — one derivation "
        "over the window, not two numbers that happen to agree",
    )
    assert_eq(
        (toolbar1[0], toolbar1[2]),
        (canvas1[0], canvas1[2]),
        "B: ★ and the toolbar still spans the canvas's column. It read the "
        "OPENING widths until R1887, which is the divergence this round had to "
        "repair a second time inside the inspector",
    )
    ok(
        f"B: ★★★★★ and the pane's own BODY grew with it — {body0[2]} -> "
        f"{body1[2]}. Ten rectangles inside this pane stated their width from "
        f"the opening constant until this round, which is a defect whose date "
        f"is the round that builds the drag",
        body1[2] > body0[2],
    )
    ok(
        f"B: the width landed inside the declared {bounds}",
        bounds["min"] <= pane1[2] <= bounds["max"],
    )


def section_c(app: RpcSubprocess, declared: dict) -> None:
    banner("C — the width reaches a reader who never sees the drawing")
    bounds = declared[PANE]["resize"]
    painted = rects(app)

    grip = nodes_by_tag(app).get(f"{PANE}.grip")
    ok("C: the grip is in the assembled application's accessibility tree", grip is not None)
    assert_eq(
        grip["role"],
        "slider",
        "C: ★★★ published as a SLIDER, which is what a resize grip is — one "
        "value between two bounds. A button would publish the gesture and drop "
        "the numbers, and the numbers are the whole of what this reader needs",
    )
    # The wire nests a numeric value under `float`, which is what carries the
    # `valuenow` / `valuemin` / `valuemax` triple together — the arrangement
    # that makes the three impossible to publish out of step.
    value = (grip.get("value") or {}).get("float") or {}
    assert_eq(
        [value.get("min"), value.get("max")],
        [float(bounds["min"]), float(bounds["max"])],
        "C: ★★★★★ carrying the SAME bounds the specification declares — the "
        "paint, the wire and the accessibility tree are three publications of "
        "one declaration and cannot drift apart",
    )
    assert_eq(
        value.get("value"),
        float(painted[PANE][2]),
        "C: ★★ and the value is the width the frame actually has, after the "
        "drag — not the width it opened at",
    )


def section_d(app: RpcSubprocess, surface: str, declared: dict) -> None:
    banner("D — over the wire, a width outside the range is refused WITH the range")
    bounds = declared[PANE]["resize"]

    # R1890 — driven in the ASSEMBLED application, at the address the roster
    # publishes for this seat. R1889 ran this in a SECOND PROCESS on the finding
    # that a guest's wire surface does not survive mounting; re-measured, what
    # did not survive was the root short-circuit, and the surface answers at its
    # own address. See this module's header.
    #
    # ⚠ `place` is an ACTION, so it is reached with `invoke`. The first draft
    # of this section sent it through `intervene` and caught the resulting
    # `PathIsAnAction` in a bare `except`, where it read as a policy refusal —
    # and the assertion below is what caught that, because a transport error
    # does not carry the bounds. ★ A test that treats *any* raised thing as the
    # refusal it hoped for is not testing the refusal: it passes for a screen
    # that never implemented the verb.
    for asked in (bounds["min"] - 1, bounds["max"] + 1):
        try:
            answered = app.invoke(f"{surface}/place", f"inspector,width={asked}")
        except RpcError as refusal:
            said = str(refusal)
            ok(
                f"D: ★★★★★ `width={asked}` is REFUSED and the refusal names "
                f"the range, so a caller knows what to ask instead",
                str(bounds["min"]) in said and str(bounds["max"]) in said,
            )
        else:
            ok(
                f"D: asking for {asked} must be refused, not answered "
                f"{answered!r}",
                False,
            )

    inside = (bounds["min"] + bounds["max"]) // 2
    answered = app.invoke(f"{surface}/place", f"inspector,width={inside}")
    ok(
        f"D: ★★ and a width inside the range goes through, with the verb "
        f"answering what it changed — {answered!r}",
        str(inside) in str(answered),
    )

    # The rail declares no resize at all: a different refusal, named.
    ok(
        "D: the rail declares no resize, so it is not a pane this verb sizes",
        declared["lab.rail"].get("resize") is None,
    )


def section_e(app: RpcSubprocess, declared: dict) -> None:
    banner("E — a drag past the bound CLAMPS where the wire call was refused")
    bounds = declared[PANE]["resize"]
    edge = declared[PANE]["at"]["edge"]

    painted = rects(app)
    gx, gy, gw, gh = painted[f"{PANE}.grip"]
    start = (gx + gw // 2, gy + gh // 2)
    overshoot = bounds["max"] * 2
    end = (
        (start[0] + overshoot, start[1])
        if edge == "left"
        else (max(1, start[0] - overshoot), start[1])
    )

    app.drag(from_at=start, to_at=end, steps=8)
    app.tick_ms(16)

    now = rects(app)[PANE][2]
    assert_eq(
        now,
        bounds["max"],
        "E: ★★★★★ a hand that slides far past the maximum gets the MAXIMUM — "
        "not a refusal and not a runaway panel. The same declaration that "
        "refuses the wire clamps the drag, which is why the bound lives in one "
        "place instead of being written once in the policy and once in the "
        "gesture",
    )
    grip = nodes_by_tag(app).get(f"{PANE}.grip")
    assert_eq(
        ((grip.get("value") or {}).get("float") or {}).get("value"),
        float(bounds["max"]),
        "E: and the published value followed the clamp, so a reader is told "
        "where it actually stopped",
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/nav"), SEAT, "the journey reached the node lab")
        # R1890 — one process. The address is asked for rather than composed,
        # and the declaration is read through it.
        surface = surface_of(app, SEAT)
        declared = declaration(app, surface)
        section_a(app, declared)
        section_b(app, declared)
        section_c(app, declared)
        section_d(app, surface, declared)
        section_e(app, declared)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1889 a panel width is a value a hand can drag", body)
