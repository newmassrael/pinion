#!/usr/bin/env python3
"""R1915 §5.32 §5.12 §2 #2 §2 #7 — **a member pin a split put on the frame can
be pressed, dragged from, and wired to.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives `debt-node-system-coverage-campaign` through
the debt R1914's own closing audit opened: R1914 built the split, drew the
member pins and announced them, and **a gesture could not name one**.

# ★★★★★ The defect this closes, measured rather than assumed

The hit carried a `bool` — *dial or accept* — so there was structurally nowhere
to put which member was pressed. And the tag parser split
`lab.pin.<card>.<pin>.<member>` at the LAST dot, so it read the member word as
the side, matched neither `dial` nor `accept`, and answered `Nothing`. A pin
that was painted, announced, and reachable by nothing.

That is R1890's class arriving on the hit axis: the surface was not missing, the
ADDRESS was. The model could already do the thing — a member is a real resolved
port, so a wire lands on it by index — and only the gesture could not say which.

# ⚠ What re-measurement changed about this walk's own shape

No pair of cards on the OPENING GRAPH can take this wire. Its three unwired dial
pins all belong to cards its one unwired accept pin already reaches, so every
attempt is refused *that link would close a cycle* — the document's rule doing
its job. So the walk ADDS a card, which is a gesture the screen has, rather than
weakening what it asserts until the opening graph happens to satisfy it.

# What this walk holds

  (A) the assembled tool mounts the lab; a card's dial pin splits, and both
      halves are on the frame.
  (B) ★★★★★ two drags from the SAME PIN onto the SAME TARGET, differing only
      in which half they start on, do different things — the service half is
      refused and the host half lands. A hit carrying `dial or accept` makes
      those two gestures identical, so this pair is the whole test.
  (C) the matching half lands, and the link count says so.
  (D) the halves are type-checked at the other end too: a half dropped on a
      WHOLE accept pin is refused, because a host name is not a locator.
  (E) folding the pin the wire landed on CUTS that wire, and the count of
      links falls — the cost a fold has, which the reference's `void` command
      cannot report.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1915_a_member_pin_takes_a_wire.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

#: The member words a locator is made of, written down rather than read off the
#: screen (R1698's rule). The lab's own
#: `r1914_the_published_pin_addresses_are_the_taxonomys_members` holds these to
#: the taxonomy.
PARTS = ("host", "service")

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


def cards(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/cards"))


def boxes(app: RpcSubprocess) -> dict:
    """Every painted tag's rectangle, in window coordinates."""
    return abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))


def centre(box) -> tuple[int, int]:
    return (box[0] + box[2] // 2, box[1] + box[3] // 2)


def links_of(app: RpcSubprocess, surface: str) -> list:
    return js(app.query(f"{surface}/links"))


def add_card(app: RpcSubprocess, surface: str, role: str) -> str:
    """Press the palette's row for `role` and answer the card it put down."""
    before = set(cards(app, surface))
    seat = boxes(app)[f"lab.palette.role.{role}"]
    app.click(at=centre(seat))
    app.tick_ms(16)
    return next(name for name in cards(app, surface) if name not in before)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — a dial pin splits, and both halves are on the frame")
        published = cards(app, surface)
        dialler = next(
            name
            for name, row in sorted(published.items())
            if row["pins"]["splits"].get("dial") == "yes"
        )
        app.invoke(f"{surface}/split_pin", f"{dialler},dial")
        app.tick_ms(16)
        frame = boxes(app)
        member_tags = [f"lab.pin.{dialler}.dial.{part}" for part in PARTS]
        ok(
            f"A: {dialler}'s dial halves are painted — {member_tags}",
            all(tag in frame for tag in member_tags),
        )
        ok(
            "A: and the parent is not, because it is split",
            f"lab.pin.{dialler}.dial" not in frame,
        )

        banner("B/C — WHICH half the drag starts on decides what happens")
        # A card nothing has dialled, so the wire cannot close a cycle. The
        # palette is how a person adds one, so it is how this walk adds one.
        listener = add_card(app, surface, "Store")
        app.invoke(f"{surface}/split_pin", f"{listener},accept")
        app.tick_ms(16)
        frame = boxes(app)
        target = f"lab.pin.{listener}.accept.{PARTS[0]}"
        ok(f"B: the new card's host half is on the frame ({target})", target in frame)

        # ★★★★★ THE OBSERVATION A BOOLEAN COULD NOT PRODUCE. Both drags start on
        # the same PIN and end in the same place; only which HALF they start on
        # differs. A hit that carried `dial or accept` would make these two
        # gestures identical, so the pair is the whole test — one lands, one is
        # refused by the type rule, and nothing about the endpoints tells them
        # apart.
        held = len(links_of(app, surface))
        app.drag(
            from_at=centre(frame[member_tags[1]]),
            to_at=centre(frame[target]),
        )
        app.tick_ms(16)
        ok(
            f"B: a SERVICE half dragged onto a HOST half makes no link — "
            f"{held} unchanged",
            len(links_of(app, surface)) == held,
        )

        banner("C — and the matching half lands")
        app.drag(
            from_at=centre(frame[member_tags[0]]),
            to_at=centre(frame[target]),
        )
        app.tick_ms(16)
        after = links_of(app, surface)
        ok(
            f"C: ★★★★★ the HOST half onto the host half made a link — "
            f"{held} -> {len(after)}, from the same pin and onto the same "
            f"target as the drag that did not",
            len(after) == held + 1,
        )

        banner("D — the halves are type-checked")
        # A second card, unsplit, so its accept pin is a WHOLE locator.
        whole = add_card(app, surface, "Store")
        frame = boxes(app)
        held = len(links_of(app, surface))
        app.drag(
            from_at=centre(frame[member_tags[1]]),
            to_at=centre(frame[f"lab.pin.{whole}.accept"]),
        )
        app.tick_ms(16)
        ok(
            "D: a service half dropped on a WHOLE accept pin makes no link — a "
            "half of a locator is not a locator, and the taxonomy says so",
            len(links_of(app, surface)) == held,
        )

        banner("E — folding the pin the wire landed on cuts the wire")
        said = app.invoke(f"{surface}/split_pin", f"{listener},-accept.{PARTS[0]}")
        app.tick_ms(16)
        ok(f"E: the fold answered — {said}", "back together" in said)
        ok(
            f"E: ★★★★★ and the wire is gone — {len(after)} -> "
            f"{len(links_of(app, surface))}; a member port that stops existing "
            f"takes what landed on it, and the tool is what says so",
            len(links_of(app, surface)) == len(after) - 1,
        )
        row = cards(app, surface)[listener]["pins"]
        ok("E: the accept pin is whole again", row["accept"] == "drawn")
        ok("E: with no members", row["members"]["accept"] == [])

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1915_a_member_pin_takes_a_wire", body))
