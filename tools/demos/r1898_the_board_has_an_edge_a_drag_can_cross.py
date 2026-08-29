#!/usr/bin/env python3
"""R1898 — the board has an edge, and a drag crosses it in both directions.

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the campaign
`debt-the-arrangeable-unit-is-a-panel-and-should-be-an-area`'s order step 2 —
the board adopting the dock layer's drag-docking — after R1891 took the float
half of it.

# The fork this closes, measured before the round

Driving the assembled tool at R1898: a card gripped on the board and carried
off it answered `Dropped::Abandoned`, which this tree's own comment glosses as
"nothing happened, and nothing was wrong". A detached panel dragged back over
the board slid across it and came to rest on top. BOTH crossings existed as
controls — the tear-off mark and the re-dock mark — and NEITHER existed as a
gesture, so where a card lived could be changed and where it landed could not
be chosen.

# What the value is, and why the floor cannot answer it

`pinion_core::crossing` says which side of a container's edge a release would
land on, as one value the preview and the release both read. Read from the
floor toolkit's own 6.11.1 sources: its detachable-panel class publishes 24
members and 5 signals and NONE answers where a release would put the panel; the
words a prospective placement would be named with occur zero times across the
four files that implement its docking. It does have this module's one bit — a
flag on its drag state, set from a held modifier key, that three sites branch on
to skip the dock — and keeps it private, carrying no sentence.

So this walk's claim is not "it docks". It is: **the application will tell you
what letting go would do, and when the answer is nothing it says which gesture
would have worked.**

# What this walk holds

  (A) at rest the slot is null, and the board holds its cards.
  (B) a card carried off the board is announced as leaving BEFORE the release,
      and comes to rest where the pointer let go, at the size it had.
  (C) dragging that panel by its body back over the board is REFUSED, in a
      sentence naming the gesture that works — the floor's private bit,
      published.
  (D) dragging its re-dock mark onto a cell previews that cell and docks there.
  (E) a press on the same mark still re-docks at the bottom: one control, two
      gestures, and the action the canon has is not taken away.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1898_the_board_has_an_edge_a_drag_can_cross.py
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

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rects(app: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    return abs_rects_of(app.snapshot(source="paint"))


def centre(rect: tuple[int, int, int, int]) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


def tiles(app: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """The board, as id -> (col, row, w, h)."""
    board = json.loads(app.query(f"{EXT}/layout"))
    return {t["id"]: (t["col"], t["row"], t["w"], t["h"]) for t in board["tiles"]}


def floats(app: RpcSubprocess) -> dict[str, dict]:
    return {f["id"]: f for f in app.query(f"{EXT}/floats")}


def crossing(app: RpcSubprocess):
    return app.query(f"{EXT}/crossing")


def settle(app: RpcSubprocess) -> None:
    for _ in range(4):
        app.tick_ms(16)


class Home:
    """Where the card this walk carries starts: its cell, and its rectangle."""

    def __init__(self, cell, rect, grip):
        #: (col, row, w, h) in the board's own units.
        self.cell = cell
        #: (x, y, w, h) in window pixels.
        self.rect = rect
        #: The centre of its grip, which is what a hand grabs.
        self.grip = grip

    @property
    def at(self):
        """A window point inside the card's FIRST cell.

        Its top-left corner rather than its centre, and that is the pointer's
        anchor rather than a preference: a carry puts the footprint's top-left
        in the cell under the cursor, exactly as a palette carry does, so aiming
        at the centre of a four-column card asks for a cell two columns along.
        The first draft of this walk aimed at the centre and read back column 1,
        which is the carry answering correctly.
        """
        return (self.rect[0] + 20, self.rect[1] + 20)


def section_a(app: RpcSubprocess) -> Home:
    banner("A — at rest there is no gesture, and the board holds its cards")
    ok(
        f"A: the crossing slot is null when nothing is in flight — "
        f"{crossing(app)!r}",
        crossing(app) is None,
    )
    cell = tiles(app)[CARD]
    ok(f"A: ★ the card this walk carries is on the board at {cell}", cell[1] == 0)
    assert_eq(app.query(f"{EXT}/floats"), [], "A: and nothing is detached yet")
    shot = rects(app)
    ok(
        "A: the board paints the card and its grip, so a hand can reach it",
        f"card.{CARD}" in shot and f"card.{CARD}.grip" in shot,
    )
    return Home(cell, shot[f"card.{CARD}"], centre(shot[f"card.{CARD}.grip"]))


def off_the_board(app: RpcSubprocess) -> tuple[float, float]:
    """A point inside the application and outside the board: the palette."""
    return centre(rects(app)[f"shell.palette.{CARD.split('#')[0]}"])


def section_b(app: RpcSubprocess, home: Home) -> None:
    banner("B — a card carried off the board leaves it, where it was let go of")
    away = off_the_board(app)
    app.drag(from_at=home.grip, to_at=away, steps=8, phase="begin")
    held = crossing(app)
    ok(
        f"B: ★★★★★ the application says what letting go would do BEFORE the "
        f"release — {held!r}",
        held is not None
        and held["began"] == "inside"
        and held["rest"]["side"] == "outside"
        and held["verdict"]["passage"] == "left"
        and held["verdict"]["crosses"] is True,
    )
    ok(
        "B: ★ and it names the card in words rather than by id, because the "
        "sentence is for a person",
        held["unit"] == "Message Stream",
    )
    app.drag(from_at=away, to_at=away, steps=1, phase="end")
    settle(app)

    ok(f"B: ★★ the card has left the board — {sorted(tiles(app))}", CARD not in tiles(app))
    panel = floats(app).get(CARD)
    ok(f"B: ★★ and it is a panel on the canvas — {panel!r}", panel is not None)
    ok(
        "B: ★ on the CANVAS, not in a window: a drag ends at a point on this "
        "canvas, and a window server has no use for that coordinate",
        panel["home"] == "canvas",
    )
    # The geometry transfer R1891 named and left open.
    ok(
        f"B: ★★★★★ it arrives at the SIZE it had on the board — panel "
        f"{panel['w']}x{panel['h']}, cell {home.rect[2]}x{home.rect[3]}",
        (panel["w"], panel["h"]) == (home.rect[2], home.rect[3]),
    )
    ok(
        "B: ★ and the crossing is over, so the slot is null again",
        crossing(app) is None,
    )


def section_c(app: RpcSubprocess, home: Home) -> None:
    banner("C — dragging the panel by its body does NOT dock it, and says why")
    home_at = home.at
    panel = centre(rects(app)[f"float.{CARD}"])
    app.drag(from_at=panel, to_at=home_at, steps=8, phase="begin")
    held = crossing(app)
    ok(
        f"C: ★★★★★ the gesture declares that it does not cross, and the "
        f"refusal names the one that does — {held!r}",
        held is not None
        and held["policy"] == "stays"
        and held["verdict"]["passage"] is None
        and held["verdict"]["crosses"] is False
        and held["verdict"]["refused"] == "may-not-join",
    )
    ok(
        f"C: ★★ the sentence tells a person what to do instead — "
        f"{held['verdict']['because']!r}",
        "re-dock mark" in held["verdict"]["because"],
    )
    ok(
        "C: ★★★★★ and the pointer IS over the board — the refusal is the "
        "declaration's, not an accident of which panel is painted on top",
        held["rest"]["side"] == "inside",
    )
    app.drag(from_at=home_at, to_at=home_at, steps=1, phase="end")
    settle(app)
    ok(
        f"C: ★★ so the card is still detached — {sorted(tiles(app))}",
        CARD not in tiles(app) and CARD in floats(app),
    )


def section_d(app: RpcSubprocess, home: Home) -> None:
    banner("D — dragging its re-dock mark onto a cell docks it THERE")
    home_at = home.at
    mark = centre(rects(app)[f"float.{CARD}.redock"])
    app.drag(from_at=mark, to_at=home_at, steps=8, phase="begin")
    held = crossing(app)
    ok(
        f"D: ★★★★★ the same slot now says the release would put it in — "
        f"{held!r}",
        held is not None
        and held["began"] == "outside"
        and held["policy"] == "crosses"
        and held["verdict"]["passage"] == "joined"
        and held["verdict"]["crosses"] is True,
    )
    ok(
        "D: ★★ and the board is DRAWING that cell, so what a person sees and "
        "what the release reads are one value",
        "shell.carry.slot" in rects(app),
    )
    app.drag(from_at=home_at, to_at=home_at, steps=1, phase="end")
    settle(app)

    back = tiles(app).get(CARD)
    ok(f"D: ★★ the card is on the board again — {back}", back is not None)
    ok(
        f"D: ★★★★★ in the cell UNDER THE POINTER, which here is the cell it "
        f"came from — back at {back[:2]}, it left {home.cell[:2]}",
        back[:2] == home.cell[:2],
    )
    ok(f"D: ★ and nothing is detached — {app.query(f'{EXT}/floats')}", not floats(app))


def section_e(app: RpcSubprocess) -> None:
    banner("E — a PRESS on the same mark still re-docks at the bottom")
    app.invoke(f"{EXT}/act", f"{CARD},tear_off")
    app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    settle(app)
    ok(f"E: the card is a canvas panel again — {sorted(floats(app))}", CARD in floats(app))

    mark = centre(rects(app)[f"float.{CARD}.redock"])
    app.click(mark)
    settle(app)
    back = tiles(app).get(CARD)
    ok(f"E: ★★ the press put it back — {back}", back is not None)
    ok(
        f"E: ★★★★★ at the BOTTOM of the board, which is what the behaviour "
        f"canon does — the drag chose a cell and the press did not, and this "
        f"round did not take the press away — row {back[1]}",
        back[1] > 0,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        home = section_a(app)
        section_b(app, home)
        section_c(app, home)
        section_d(app, home)
        section_e(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1898 the board has an edge a drag can cross", body)
