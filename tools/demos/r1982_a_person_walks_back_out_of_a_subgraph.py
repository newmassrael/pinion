#!/usr/bin/env python3
"""R1982 §5.2 §5.11 — **a person goes into a subgraph and PRESSES their way back
out**, and the palette puts a card where they are standing.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981 gave the assembled tool the descent; this drives the half that makes the
descent something a person can survive.

# ★★★★★ The sharp half of the debt, and why it is about a person

`debt-the-assembled-tools-subgraph-surface-is-half-built` listed four missing
capabilities and named this one sharpest: the breadcrumb R1981 drew SAID where
a person was and could not be pressed, and `exit` was reachable from the wire
and from nothing on the frame. So a person with a pointer or a keyboard could
enter a subgraph and **not come out**. The other three are things the tool
cannot do; this one is a room with no door, which is a different kind of wrong.

Each step of the way in is a control now, so a person can climb one level or
several — which is what the reference's own path does.

# ⚠ Two defects R1981 left that only driving found

* **The palette added a card to the ROOT** whatever tree was on screen, so from
  inside a subgraph a person pressed a palette row and nothing appeared. R1981's
  own ratchet could not see the site: it matched `(ROOT` and the token sits on a
  line of its own inside a wrapped call. The gate has no discard path now —
  measured, it saw 12 of 25 tokens — and it named three sites, of which this and
  `set_port_address` were real.
* **An address written beside a wire went to the root's card of that number.**
  Same cause, same repair.

# What this walk holds

  (A) the journey reaches the node lab, at the top, with nowhere above.
  (B) ★ two cards are folded and entered — R1981's capability, as the setup.
  (C) ★★★★★ the frame now carries a PRESSABLE step for the tree above, and
      pressing it stands the person there. This is the door.
  (D) ★ the step a person is standing on is NOT a control — a chip that did
      nothing when pressed would be worse than no chip.
  (E) ★★★★★ the palette adds its card to the tree ON SCREEN, not to the root.
  (F) ★ and an address written inside lands on the card inside.
  (G) ★★★★★ from two levels down, one press climbs all the way to the top —
      the affordance a repeated `exit` does not give.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1982_a_person_walks_back_out_of_a_subgraph.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, abs_rects_of, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def standing(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/standing"))


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def crumb_marks(app: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Every breadcrumb chip on the frame, by tag.

    Read off the FRAME rather than off the wire: the claim is that a person
    looking at the screen has somewhere to press, and only the paint says
    whether a control is there to be pressed.
    """
    marks = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
    return {
        tag: rect
        for tag, rect in marks.items()
        if tag == "lab.crumb" or tag.startswith("lab.crumb.up.")
    }


def press(app: RpcSubprocess, rect: tuple[int, int, int, int]) -> None:
    x, y, w, h = rect
    app.click((x + w // 2, y + h // 2))
    app.tick_ms(16)


def fold_and_enter(app: RpcSubprocess, surface: str, name: str) -> None:
    here = cards(app, surface)
    app.invoke(f"{surface}/select", here[0])
    app.tick_ms(16)
    app.invoke(f"{surface}/select_also", here[1])
    app.tick_ms(16)
    app.invoke(f"{surface}/group", name)
    app.tick_ms(16)
    app.invoke(f"{surface}/enter", name)
    app.tick_ms(16)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "A: the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)
        at = standing(app, surface)
        ok(f"A: ★ it opens at the top — {at}", at["depth"] == 0)
        top = crumb_marks(app)
        ok(
            f"A: ★★★★★ and at the top there is NO step to press, because there "
            f"is nowhere above — {sorted(top)}",
            sorted(top) == ["lab.crumb"],
        )

        banner("B — two cards are folded and entered (R1981's capability)")
        fold_and_enter(app, surface, "capture-side")
        at = standing(app, surface)
        ok(f"B: ★ one descent deep — {at}", at["depth"] == 1)

        banner("C — ★★★★★ the frame carries a door, and pressing it opens it")
        marks = crumb_marks(app)
        ok(
            f"C: ★★★★★ a PRESSABLE step for the tree above is on the frame — "
            f"{sorted(marks)}. Before R1982 the breadcrumb only SAID where a "
            "person was, so a pointer could go in and not come out",
            "lab.crumb.up.0" in marks,
        )
        press(app, marks["lab.crumb.up.0"])
        at = standing(app, surface)
        ok(
            f"C: ★★★★★ and the press puts them back at the top — {at}",
            at["depth"] == 0 and at["inside"] is False,
        )

        banner("D — ★ the step you are standing on is not a control")
        # Pressing the current step must not be a control that does nothing:
        # what it does is fall through to the canvas, which is what the rest of
        # the chip's area does.
        before = standing(app, surface)
        press(app, crumb_marks(app)["lab.crumb"])
        ok(
            f"D: ★ pressing where you already are changes nothing about where "
            f"you are — {before} then {standing(app, surface)}",
            standing(app, surface)["depth"] == before["depth"],
        )

        banner("E — ★★★★★ the palette adds its card to the tree ON SCREEN")
        app.invoke(f"{surface}/enter", "capture-side")
        app.tick_ms(16)
        inside_before = cards(app, surface)
        # ⚠ PRESSED, not invoked. Adding a card from the palette is not a wire
        # verb on this screen — it is a row a person presses — so driving it any
        # other way would be asserting about a path a person does not have.
        # ★ R2049 — the addresses the screen publishes, not a prefix spelled
        # here. Asked once rather than per tag.
        offered = {row["tag"] for row in js(app.query(f"{surface}/spec"))["roles"]}
        rows = {
            tag: rect
            for tag, rect in abs_rects_of(
                app.snapshot(source="paint", viewport=VIEWPORT)
            ).items()
            if tag in offered
        }
        ok(f"E: ★ the palette offers rows to press — {sorted(rows)}", rows)
        press(app, rows[sorted(rows)[0]])
        inside_after = cards(app, surface)
        ok(
            f"E: ★★★★★ the card appears where the person is standing — "
            f"{len(inside_before)} then {len(inside_after)}. Until R1982 this "
            "went to the ROOT and the canvas did not change",
            len(inside_after) == len(inside_before) + 1,
        )
        made = [name for name in inside_after if name not in inside_before]
        ok(f"E: ★ and it is one card, named — {made}", len(made) == 1)

        banner("F — ★ an address written inside lands inside")
        app.invoke(f"{surface}/select", made[0])
        app.tick_ms(16)
        # The card is here; the point is that writing does not reach out to the
        # root's card of the same number.
        root_before = None
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        root_before = cards(app, surface)
        app.invoke(f"{surface}/enter", "capture-side")
        app.tick_ms(16)
        ok(
            f"F: ★ the root's roster is unchanged by what happened inside — "
            f"{len(root_before)} card(s) out there",
            made[0] not in root_before,
        )

        banner("G — ★★★★★ one press climbs several levels")
        fold_and_enter(app, surface, "inner")
        at = standing(app, surface)
        ok(f"G: ★ two descents deep — {at['through']}", at["depth"] == 2)
        marks = crumb_marks(app)
        ok(
            f"G: ★ a step is offered for EACH tree above — {sorted(marks)}",
            "lab.crumb.up.0" in marks and "lab.crumb.up.1" in marks,
        )
        press(app, marks["lab.crumb.up.0"])
        at = standing(app, surface)
        ok(
            f"G: ★★★★★ and pressing the first climbs ALL the way, which a "
            f"repeated exit gives one level at a time — {at}",
            at["depth"] == 0,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1982 a person walks back out of a subgraph", body)
