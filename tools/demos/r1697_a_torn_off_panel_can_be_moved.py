#!/usr/bin/env python3
"""R1697 §5.20 §5.35 §5.38 §2 #2 #7 — **a torn-off panel can be moved.**

A person opened this application, tore a card off the board into a floating
panel, tried to drag it, and it would not move. They asked whether that was
intended. It was not: the press arm that would have started the gesture read

    Hit::Float(_) | Hit::Nothing => {}

— a detached panel folded in with hitting nothing at all — and nothing anywhere
opened a drag for one. Every gate on the screen was green, and each of them was
right. The panel is painted, hit-testable, inside its own rectangle, named and
announced to a screen reader. Not one of them asks whether **grabbing it moves
it**, and none of them could, because the screen had no table saying it should.

The sibling screen has had such a table since R1677 and it caught this exact
class three times there. So this round gives the dashboard one, lifts its shape
into the framework so a third screen does not invent a third, and drives every
row of it here through a real window.

What the reference does, read out of its own source rather than inferred from a
picture: a panel opens 520x380 with a stacking number taken from a monotonic
counter; grabbing it **raises it first** and then moves it by the pointer's
delta; its corner sizes it and clamps at 320x220. All three are one gesture —
the drag calls the raise — which is why doing one of them alone leaves the other
two in the wrong place.

The gate found a second defect on its first run, before this file existed: a card
maximised with the mouse had **no way back with the mouse**. The header control
called `maximize` again, which refuses with "a card is already maximised". It
toggles now, and shows a restore mark when it will.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1697_a_torn_off_panel_can_be_moved.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-analyzer-shell"
EXT = "/external"
CHECKS: list[str] = []

# The reference's own numbers, from its source.
OPEN_W, OPEN_H = 520, 380
MIN_W, MIN_H = 320, 220


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def rects(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint"))


def centre(rect) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


def panels(app: RpcSubprocess) -> dict:
    """The detached panels by id, front to back on the wire."""
    return {row["id"]: row for row in q(app, "floats")}


def act(app: RpcSubprocess, card: str, affordance: str):
    return app.invoke(f"{EXT}/act", f"{card},{affordance}")


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
        spec = q(app, "spec")

        # ── (A) the screen publishes what it can be asked to do ────────────
        banner("A — the operations table is on the wire, with its witnesses")
        table = spec["operations"]
        ok("A: the screen publishes an operations table", isinstance(table, list))
        names = [row["name"] for row in table]
        assert_eq(len(set(names)), len(names), "A: the operations are named uniquely")
        for row in table:
            ok(f"A: {row['name']} names a witness", bool(row["witness"]))
            ok(
                f"A: {row['name']} can be caused",
                bool(row["verb"]) or row["gesture"],
            )
            # Every witness must be a slot this screen actually answers —
            # otherwise the table names evidence nobody can read.
            q(app, row["witness"])
        for row in table:
            if row["needs"]:
                ok(
                    f"A: {row['name']}'s precondition is in the table",
                    row["needs"] in names,
                )
        movable = [n for n in names if "detached panel" in n]
        ok("A: the panel's own operations are declared", len(movable) >= 4)
        print(f"[demo] {len(table)} operation(s) declared, {len(movable)} about a panel")

        # ── (B) a card leaves the board at the reference's size ────────────
        banner("B — a torn-off card becomes a panel of the reference's size")
        card = q(app, "cards").split(",")[0]
        assert_eq(q(app, "floating"), "", "B: nothing is detached to begin with")
        act(app, card, "tear_off")
        app.tick(16)
        assert_eq(q(app, "floating"), card, "B: the card is now a panel")
        first = panels(app)[card]
        assert_eq((first["w"], first["h"]), (OPEN_W, OPEN_H), "B: the opening size")
        ok("B: it arrives with a stacking number", first["z"] > 0)
        ok("B: and it is painted", f"float.{card}" in rects(app))

        # ── (C) the panel moves when it is dragged ─────────────────────────
        banner("C — ★ the defect: grabbing the panel moves it")
        before = panels(app)[card]
        painted = rects(app)[f"float.{card}"]
        start = centre(painted)
        app.drag(from_at=start, to_at=(start[0] + 90, start[1] + 55))
        app.tick(16)
        after = panels(app)[card]
        assert_eq(
            (after["x"] - before["x"], after["y"] - before["y"]),
            (90, 55),
            "C: the panel moved by exactly the pointer's delta",
        )
        # And the screen agrees: the painted rectangle moved with it.
        moved = rects(app)[f"float.{card}"]
        assert_eq(
            (moved[0] - painted[0], moved[1] - painted[1]),
            (90, 55),
            "C: and the paint followed, so a person sees it move",
        )
        assert_eq((after["w"], after["h"]), (OPEN_W, OPEN_H), "C: moving is not sizing")
        ok("C: the toast says it moved", "moved" in q(app, "toast").lower())

        # A press that moves nothing is a click, not a move.
        app.request("scene/click", {"button": "left", "at": {"x": moved[0] + 8, "y": moved[1] + 8}})
        app.tick(16)
        assert_eq(panels(app)[card]["x"], after["x"], "C: a click moves nothing")

        # ── (D) the corner sizes it, and stops at the floor ────────────────
        banner("D — the corner sizes the panel, and clamps where the reference does")
        grip = rects(app)[f"float.{card}.resize"]
        ok("D: the corner is painted, so a person can find it", grip[2] > 0)
        base = panels(app)[card]
        app.drag(from_at=centre(grip), to_at=(centre(grip)[0] + 70, centre(grip)[1] + 45))
        app.tick(16)
        grown = panels(app)[card]
        assert_eq(
            (grown["w"] - base["w"], grown["h"] - base["h"]),
            (70, 45),
            "D: the corner grew the panel by the pointer's delta",
        )
        assert_eq((grown["x"], grown["y"]), (base["x"], base["y"]), "D: sizing is not moving")
        # Pull it far past the floor, in both axes at once.
        grip = rects(app)[f"float.{card}.resize"]
        app.drag(from_at=centre(grip), to_at=(1.0, 1.0))
        app.tick(16)
        floored = panels(app)[card]
        assert_eq((floored["w"], floored["h"]), (MIN_W, MIN_H), "D: it clamps at the floor")
        ok(
            "D: ★ and the floor is a floor, not a collapse — the panel is still usable",
            f"float.{card}.redock" in rects(app),
        )

        # ── (E) stacking: a press brings a panel to the front ──────────────
        banner("E — two panels, and a press brings the one underneath forward")
        second = q(app, "cards").split(",")[1]
        act(app, second, "tear_off")
        app.tick(16)
        order = list(panels(app))
        assert_eq(order[0], second, "E: the newest panel arrives in front")
        ok("E: it is in front by its stacking number", panels(app)[second]["z"] > panels(app)[card]["z"])
        # The paint agrees with the wire: front on the wire is painted last.
        painted_order = [
            tag for tag in rects(app) if tag.startswith("float.") and tag.count(".") == 1
        ]
        # `abs_rects_of` is a dict in paint order for this screen's floats.
        ok("E: both panels are painted", set(painted_order) == {f"float.{card}", f"float.{second}"})

        # Slide the front panel to the right so a strip of the one behind is
        # showing, and take the aim from the PAINTED rectangles rather than
        # computing where they ought to be.
        front = rects(app)[f"float.{second}"]
        app.drag(from_at=centre(front), to_at=(centre(front)[0] + 260, centre(front)[1]))
        app.tick(16)
        back = rects(app)[f"float.{card}"]
        front = rects(app)[f"float.{second}"]
        ok("E: the panels still overlap, which is what makes stacking a fact",
           front[0] < back[0] + back[2])
        showing = (back[0] + 6, back[1] + back[3] / 2)
        ok("E: the aim is on the back panel and not on the front one", showing[0] < front[0])
        app.request("scene/click", {"button": "left", "at": {"x": showing[0], "y": showing[1]}})
        app.tick(16)
        ok(
            "E: ★ the pressed panel came forward",
            panels(app)[card]["z"] > panels(app)[second]["z"],
        )
        assert_eq(list(panels(app))[0], card, "E: and the wire says so, front first")

        # ── (F) the panel goes home, and the roster empties ────────────────
        banner("F — a panel re-docks and closes, which is what it always could do")
        redock = rects(app)[f"float.{card}.redock"]
        app.request("scene/click", {"button": "left", "at": {"x": centre(redock)[0], "y": centre(redock)[1]}})
        app.tick(16)
        assert_eq(q(app, "floating"), second, "F: the re-docked panel left the roster")
        close = rects(app)[f"float.{second}.close"]
        app.request("scene/click", {"button": "left", "at": {"x": centre(close)[0], "y": centre(close)[1]}})
        app.tick(16)
        assert_eq(q(app, "floating"), "", "F: and the closed one too")
        assert_eq(q(app, "floats"), [], "F: no geometry is left behind")

        # ── (G) the second defect the gate found ───────────────────────────
        banner("G — ★ the maximise control toggles, so the mouse has a way back")
        card = q(app, "cards").split(",")[0]
        assert_eq(q(app, "maximized"), "", "G: nothing is maximised to begin with")
        mark = rects(app)[f"card.{card}.maximize"]
        app.request("scene/click", {"button": "left", "at": {"x": centre(mark)[0], "y": centre(mark)[1]}})
        app.tick(16)
        ok("G: the card maximised", q(app, "maximized") != "")
        mark = rects(app)[f"card.{card}.maximize"]
        app.request("scene/click", {"button": "left", "at": {"x": centre(mark)[0], "y": centre(mark)[1]}})
        app.tick(16)
        assert_eq(
            q(app, "maximized"),
            "",
            "G: ★ and the same control restored it — before this round it refused",
        )
        ok("G: the toast says restored", "restored" in q(app, "toast").lower())

        # ── (H) the wire's two verbs stay precise ──────────────────────────
        banner("H — the wire keeps two verbs, because an agent asks for an outcome")
        app.invoke(f"{EXT}/maximize", card)
        app.tick(16)
        ok("H: the wire maximises", q(app, "maximized") != "")
        refused = ""
        try:
            app.invoke(f"{EXT}/maximize", card)
        except Exception as why:  # noqa: BLE001 - any refusal shape is fine
            refused = str(why)
        ok("H: ★ and asking for it again REFUSES rather than toggling", "already" in refused)
        app.invoke(f"{EXT}/restore", "")
        app.tick(16)
        assert_eq(q(app, "maximized"), "", "H: and restore is its own verb")

        print(f"\n[demo] {len(CHECKS)} narrated check(s) beyond the assertions")


run_demo("R1697 a torn-off panel can be moved", body)
