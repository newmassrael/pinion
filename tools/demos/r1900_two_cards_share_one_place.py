#!/usr/bin/env python3
"""R1900 — two cards share one place, and a strip chooses between them.

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the campaign
`debt-the-arrangeable-unit-is-a-panel-and-should-be-an-area`'s order step 2 —
the board adopting the dock layer — and specifically the **one item R1898 left
open there**. That round measured the other two and struck them off: `split`
degenerates on a twelve-column tile grid (a split is a resize plus a placement,
and both already work), and drag-docking closed with `pinion_core::crossing`.
What remained was `tabs`.

# The fork this closes, measured before the round

Driving the assembled tool at R1899, a cell of the board and the card in it were
**one name**: `Tile.id` *was* the card's id, so the arrangement could say where
a card is and could not say that two cards are in the same place — the type had
no room for a second one. Every board gesture therefore had to give each card a
rectangle of its own, and a board of twelve columns is a board of at most twelve
things.

That is the same shape R1893 met one axis over: a delete could not exist because
*where an arrangement came from* was not a distinction the map carried. ⇒ ★★★★★
**a capability that is absent may be the downstream of a distinction that is
absent.**

# Why the behaviour canon does not answer it, and we build it anyway

Measured in this round by extracting the behaviour canon's inline application
(the recorded procedure) and counting: the string `tab` occurs **four** times in
194,828 characters, and every one is a coincidence of spelling — three inside a
predicate about whether a link may be edited, one inside a status word — so the
canon has no tab anything. Its board is a twelve-column tile grid whose widgets
each own a rectangle (`GRID_COLS = 12`, `ROW_H = 174`, `GAP = 16`,
re-measured this round). So this is a
second-pass improvement, which the standing order rule admits — the floor
toolkit stacks detachable panels into one region, so a consumer is assumed.

Read from that floor's own 6.11.1 headers at R1900, over the class that stacks
panels into a region and the window class that owns the operation: stacking is a
`void` call, so there is **nothing to refuse with**; the set sharing a region is
readable and **no accessor publishes which of them is in front** (only an
activation signal, after the fact); and there is **no verb for taking one back
out** — a panel leaves a stack by being added somewhere else, so "un-stack" is a
side effect of another operation rather than an act with an outcome. Here each
of those is a value — see the table on `pinion_core::stacking::Stack`.

# What this walk holds

  (A) at rest every card is in a place of its own, and no strip is drawn.
  (B) a card carried onto another card's HEADER is previewed as a join before
      the release, and the release puts them in one place — with the card just
      dropped in front, and the one it covered still reachable.
  (C) a reader meets the strip as a tab list whose selected tab is the one in
      front.
  (D) pressing the other tab brings it forward; the board's cell does not move,
      and the card that was in front is still there.
  (E) a tab dragged out onto the board gets a place of its own, at the size it
      was sharing — and the place it left keeps its last occupant.
  (F) the wire drives all three verbs, and the last occupant of a place is
      refused by name, in a sentence naming the gesture that works instead.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1900_two_cards_share_one_place.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
#: The card this walk carries, and the one it is dropped onto.
GUEST = "packet#0"
HOST = "decode#1"

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


def tiles(app: RpcSubprocess) -> dict[str, dict]:
    """The board, as the id of each cell's FRONT occupant -> the whole row."""
    board = json.loads(app.query(f"{EXT}/layout"))
    return {t["id"]: t for t in board["tiles"]}


def occupants(app: RpcSubprocess, front: str) -> list[str]:
    """Who shares the cell whose front is `front`, in strip order.

    `here` is absent for a cell with one occupant — see the note on
    `pinion_core::widgets::tile_grid::Tile`, where that absence is what lets an
    arrangement saved before this round still load — so this reads the stored
    form the way a client has to.
    """
    row = tiles(app).get(front)
    if row is None:
        return []
    return list(row.get("here") or [row["id"]])


def access(app: RpcSubprocess) -> dict:
    """The structured NAME channel, which is a different tree from the paint."""
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result


def _bounds(tree: dict, tag: str) -> tuple[int, int, int, int] | None:
    """The rectangle the NAME channel gives for `tag`, in the paint's shape."""
    node = access_node_by_tag(tree, tag)
    if node is None or "bounds" not in node:
        return None
    b = node["bounds"]
    return (b["x"], b["y"], b["w"], b["h"])


def settle(app: RpcSubprocess) -> None:
    for _ in range(4):
        app.tick_ms(16)


def header_of(app: RpcSubprocess, card: str) -> tuple[float, float]:
    """A point on `card`'s header band that is not one of its controls.

    Just right of the grip: the grip is where the header says it is dragged
    from, and the affordance slots are right-aligned, so the space between them
    is the header's own.
    """
    grip = rects(app)[f"card.{card}.grip"]
    return (grip[0] + grip[2] + 12, grip[1] + grip[3] / 2)


def section_a(app: RpcSubprocess) -> None:
    banner("A — every card is in a place of its own, and no strip is drawn")
    board = tiles(app)
    ok(f"A: both cards this walk uses are on the board — {sorted(board)}",
       GUEST in board and HOST in board)
    for card in (GUEST, HOST):
        ok(
            f"A: {card} is the only occupant of its cell — {occupants(app, card)}",
            occupants(app, card) == [card],
        )
    shot = rects(app)
    ok(
        "A: ★ nothing paints a tab, because nothing shares a place",
        not [tag for tag in shot if tag.endswith(".tab")],
    )
    ok(
        "A: and the cells are two different rectangles",
        shot[f"card.{GUEST}"] != shot[f"card.{HOST}"],
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — a card let go on another card's header joins its place")
    grip = centre(rects(app)[f"card.{GUEST}.grip"])
    onto = header_of(app, HOST)
    was = rects(app)[f"card.{HOST}"]

    app.drag(from_at=grip, to_at=onto, steps=8, phase="begin")
    held = rects(app)
    ok(
        "B: ★★★★★ the application marks the PLACE a release would join, before "
        f"the release — {sorted(t for t in held if t.startswith('shell.carry.'))}",
        "shell.carry.join" in held,
    )
    ok(
        "B: ★★ and it marks no cell, because a join takes no cell — a mark "
        "promising a placement that will not happen is worse than no mark",
        "shell.carry.slot" not in held,
    )
    join = held["shell.carry.join"]
    ok(
        "B: ★ the mark is on the host's header BAND — inside its card and only "
        f"the top of it, which is what the gesture is aimed at — join {join}, "
        f"host card {was}",
        join[0] >= was[0]
        and join[0] + join[2] <= was[0] + was[2]
        and join[1] >= was[1]
        and join[1] + join[3] <= was[1] + was[3] // 2,
    )

    app.drag(from_at=onto, to_at=onto, steps=1, phase="end")
    settle(app)

    board = tiles(app)
    ok(
        f"B: ★★★★★ the two cards are in ONE place now — {sorted(board)}",
        GUEST in board and HOST not in board,
    )
    ok(
        "B: ★★ and the card just dropped in is the one in front, because that "
        f"is what the person is looking for — {board[GUEST]['id']}",
        board[GUEST]["id"] == GUEST,
    )
    ok(
        f"B: ★★ the one it covered is still an occupant — {occupants(app, GUEST)}",
        occupants(app, GUEST) == [HOST, GUEST],
    )
    ok(
        "B: ★★★★★ and the place is the HOST's rectangle: a join moves into a "
        f"place, it does not make a new one — now {rects(app)[f'card.{GUEST}']}, "
        f"the host was {was}",
        rects(app)[f"card.{GUEST}"] == was,
    )
    ok(
        "B: ★ the card behind is not painted at all — it is not built, which is "
        "what makes the strip the only way to reach it",
        f"card.{HOST}" not in rects(app),
    )
    shot = rects(app)
    for member in (HOST, GUEST):
        ok(
            f"B: ★★ a tab is drawn for {member} — {shot.get(f'card.{member}.tab')}",
            f"card.{member}.tab" in shot,
        )
    first, second = shot[f"card.{HOST}.tab"], shot[f"card.{GUEST}.tab"]
    ok(
        "B: ★★★★★ the tabs are CONTIGUOUS and share a baseline, so no press "
        f"lands between two of them — {first} then {second}",
        first[0] + first[2] == second[0] and first[1] == second[1],
    )
    ok(
        "B: ★★★★★ and the rectangle a reader is given is the SAME rectangle — "
        "drawn, announced and pressed are one box rather than three claims "
        "about one tab",
        [_bounds(access(app), f"card.{member}.tab") for member in (HOST, GUEST)]
        == [first, second],
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — a reader meets the strip as a tab list")
    tree = access(app)
    strip = access_node_by_tag(tree, f"card.{GUEST}.tabs")
    ok(f"C: ★★ the shared place publishes a tab list — {strip!r}", strip is not None)
    ok(
        f"C: ★ its children are the occupants' tabs — {strip.get('children')}",
        strip.get("children") == [f"card.{HOST}.tab", f"card.{GUEST}.tab"],
    )
    front = access_node_by_tag(tree, f"card.{GUEST}.tab")
    behind = access_node_by_tag(tree, f"card.{HOST}.tab")
    ok(
        "C: ★★★★★ exactly one tab is selected, and it is the one in front — "
        f"{GUEST}={front.get('selected')}, "
        f"{HOST}={behind.get('selected')}",
        front.get("selected") is True and behind.get("selected") is False,
    )
    ok(
        "C: ★ and each tab is named the way a person names the card, not by id "
        f"— {behind.get('name')!r}",
        behind.get("name") == "Decode Inspector",
    )
    ok(
        "C: ★★★★★ the card BEHIND the tab has no region of its own — it is not "
        "on the screen, so announcing its rows would be telling a reader about "
        "something nobody can reach; the tab is what carries its name",
        access_node_by_tag(tree, f"card.{HOST}") is None
        and access_node_by_tag(tree, f"card.{GUEST}") is not None,
    )


def section_d(app: RpcSubprocess) -> None:
    banner("D — pressing the other tab brings it forward, and nothing moves")
    before = rects(app)[f"card.{GUEST}"]
    app.click(centre(rects(app)[f"card.{HOST}.tab"]))
    settle(app)

    board = tiles(app)
    ok(
        f"D: ★★★★★ the place now shows the other occupant — {sorted(board)}",
        HOST in board and GUEST not in board,
    )
    ok(
        f"D: ★★ and NOBODY left: both are still here — {occupants(app, HOST)}",
        occupants(app, HOST) == [HOST, GUEST],
    )
    ok(
        f"D: ★★★★★ the place did not move — {rects(app)[f'card.{HOST}']} vs "
        f"{before}",
        rects(app)[f"card.{HOST}"] == before,
    )
    ok(
        "D: ★ the strip is drawn on whichever card is in front, so both tabs "
        "are still reachable",
        f"card.{HOST}.tab" in rects(app) and f"card.{GUEST}.tab" in rects(app),
    )
    selected = access_node_by_tag(access(app), f"card.{HOST}.tab")
    ok(
        "D: ★★ and a reader is told which one moved forward",
        selected.get("selected") is True,
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — a tab dragged out onto the board gets a place of its own")
    shared = rects(app)[f"card.{HOST}"]
    # The board's OWN units, because that is what "the size it was sharing"
    # means: the painted rectangle of a card near the foot of the canvas is
    # clipped by the viewport, so a pixel comparison would be asking about the
    # scroll position rather than about the cell.
    span = tiles(app)[HOST]
    tab = centre(rects(app)[f"card.{GUEST}.tab"])
    # An empty row below everything placed: the board grows by three rows while
    # a drag is in flight, so there is somewhere to let go that is not occupied.
    below = (shared[0] + 30, shared[1] + shared[3] + 200)

    app.drag(from_at=tab, to_at=below, steps=8, phase="begin")
    app.drag(from_at=below, to_at=below, steps=1, phase="end")
    settle(app)

    board = tiles(app)
    ok(
        f"E: ★★★★★ the two cards are in two places again — {sorted(board)}",
        GUEST in board and HOST in board,
    )
    ok(
        f"E: ★★ the one that left has the place to itself — "
        f"{occupants(app, GUEST)}",
        occupants(app, GUEST) == [GUEST],
    )
    ok(
        f"E: ★★ and so does the one it left behind — {occupants(app, HOST)}",
        occupants(app, HOST) == [HOST],
    )
    out = tiles(app)[GUEST]
    ok(
        "E: ★★★★★ it arrives at the SIZE it was sharing, which is the only "
        f"size it has ever had here — {out['w']}x{out['h']} cells vs "
        f"{span['w']}x{span['h']}",
        (out["w"], out["h"]) == (span["w"], span["h"]),
    )
    ok(
        f"E: ★ and it is where the pointer let go, below what was placed — "
        f"row {out['row']}, the place it left is row {tiles(app)[HOST]['row']}",
        out["row"] > tiles(app)[HOST]["row"],
    )
    ok(
        f"E: ★ and no tab is drawn any more — "
        f"{[t for t in rects(app) if t.endswith('.tab')]}",
        not [t for t in rects(app) if t.endswith(".tab")],
    )


def section_f(app: RpcSubprocess) -> None:
    banner("F — the wire drives all three verbs, and refuses by name")
    app.invoke(f"{EXT}/share", f"{GUEST},{HOST}")
    settle(app)
    ok(
        f"F: ★★ `share` puts them in one place — {occupants(app, GUEST)}",
        occupants(app, GUEST) == [HOST, GUEST],
    )

    answer = app.invoke(f"{EXT}/reveal", HOST)
    settle(app)
    ok(
        f"F: ★★ `reveal` answers with BOTH halves, so a caller can see whether "
        f"anything moved — {answer!r}",
        answer == f"{GUEST},{HOST}",
    )
    again = app.invoke(f"{EXT}/reveal", HOST)
    ok(
        "F: ★★★★★ and revealing what is already in front is a legal, "
        f"uninteresting outcome rather than an error — {again!r}",
        again == f"{HOST},{HOST}",
    )

    refused = None
    try:
        app.invoke(f"{EXT}/unshare", f"{GUEST},0,9")
        app.invoke(f"{EXT}/unshare", f"{GUEST},0,9")
    except Exception as why:  # noqa: BLE001 — the refusal is the assertion
        refused = str(why)
    ok(
        "F: ★★★★★ the last occupant of a place cannot be taken out of it, and "
        f"the refusal names the gesture that works instead — {refused!r}",
        refused is not None
        and "only one here" in refused
        and "move the place itself" in refused,
    )
    ok(
        "F: ★ and the board is unchanged by the refusal — "
        f"{occupants(app, GUEST)}",
        occupants(app, GUEST) == [GUEST],
    )

    missing = None
    try:
        app.invoke(f"{EXT}/share", f"{GUEST},nobody#9")
    except Exception as why:  # noqa: BLE001
        missing = str(why)
    ok(
        f"F: ★★ and a card that is on no cell is refused by NAME — {missing!r}",
        missing is not None and "nobody#9" in missing,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app)
        section_e(app)
        section_f(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1900 two cards share one place", body)
