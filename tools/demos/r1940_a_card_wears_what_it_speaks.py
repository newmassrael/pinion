#!/usr/bin/env python3
"""R1940 §5.2 §5.11 — **a card wears the colour of what it speaks, and says so.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — what a node is drawn
as, answered per NODE — as the node lab is mounted in the shell.

# ★★★★★ The measurement that reversed this row's verdict

The row's own recorded reason said *a kind here gives one appearance to all its
nodes*. Re-measured, a kind gave NONE: the three colour hooks on the trait are
every one of them about a PORT (a type's colour, control's, an undecided one's),
and nothing anywhere asked a kind what its NODE looks like. The authored-colour
field's own doc promised "`None` for whatever its kind is drawn in" and there
was nothing to fall back to — a sentence with no implementation.

In the reference, three implementations exist and ALL THREE DERIVE from the
node's own state: one reads the colour tag of the definition its group instance
stands for, two read the node's chosen data type. Two defects were measured
there and are answered here: the fallback is a SECOND declaration of the same
fact with nothing checking the two agree, and BOTH consumers carry their own
copy of the choose-the-override-else-the-fixed-class expression — with the
authored colour a third path again.

⇒ one declaration on the kind, one ranking in the document, and a third arm
(*drawn like this TYPE*) that reaches the very palette a port of that type is
drawn with. The reference cannot state that correspondence: its classes and its
socket types are separate vocabularies.

# What this walk holds

  (A) the journey reaches the node lab, and every card says what its kind draws
      it as — a sentence a screen can show before anything is painted.
  (B) ★★★★★ the card's faces come from that declaration: a card wears the
      colour of the transport it speaks, and the SAME colour its pins wear.
  (C) ★★★★★ the answer is per CARD — R1937's verb gives one card another
      transport and its colour follows in the same turn, while its neighbours
      do not move.
  (D) ★★★★★ what a person authored OUTRANKS the kind, and clearing it hands the
      card back to its kind rather than to nothing.
  (E) ★ the four faces stay DERIVED from whichever colour won, so one ranking
      feeds one derivation.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1940_a_card_wears_what_it_speaks.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

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


def tints(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/tints"))["nodes"]


def card(rows: list[dict], name: str) -> dict:
    return next(row for row in rows if row["node"] == name)


def pin_colour(app: RpcSubprocess, surface: str, name: str) -> str | None:
    """The colour this card's own dial pin is drawn in (R1926's register).

    ★ Read from the OTHER register on purpose: the point of R1940's third arm
    is that a card and its pins reach ONE declaration, and two registers
    agreeing is the only way to show it from outside.
    """
    published = js(app.query(f"{surface}/inks"))
    for row in published["pins"]:
        if row["pin"] == f"{name}.dial":
            return row["ink"]
    return None


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

        banner("A — every card says what its KIND draws it as")
        rows = tints(app, surface)
        ok(f"A: the register answers — {len(rows)} card(s)", len(rows) > 1)
        ok(
            "A: ★ every card carries a `drawn` sentence, so a screen knows WHY "
            "before it paints",
            all(row.get("drawn", {}).get("says") for row in rows),
        )
        ok(
            f"A: ★ and every one of them says it is drawn like a TYPE — "
            f"{sorted({row['drawn']['says'] for row in rows})}",
            {row["drawn"]["says"] for row in rows} == {"like_type"},
        )

        banner("B — ★★★★★ the faces come from that declaration")
        subject = rows[0]["node"]
        before = card(tints(app, surface), subject)
        ok(
            f"B: ★ the card has NO authored colour and is still drawn — "
            f"tint={before['tint']!r} faces={before['faces'] is not None}",
            before["tint"] is None and before["faces"] is not None,
        )
        ok(
            f"B: ★★★★★ and the colour it wears is the colour its own dial pin "
            f"wears, which is what ONE declaration means — "
            f"{before['faces']['title']!r}",
            pin_colour(app, surface, subject) == before["faces"]["title"],
        )

        banner("C — ★★★★★ the answer is per CARD")
        others = [row["node"] for row in rows if row["node"] != subject]
        neighbour = others[0]
        neighbour_before = card(rows, neighbour)["faces"]["title"]
        app.invoke(f"{surface}/set_pin_transport", f"{subject},dial,udp")
        app.tick_ms(16)
        after = card(tints(app, surface), subject)
        ok(
            f"C: ★★★★★ the card's colour FOLLOWED the transport it now speaks — "
            f"{before['faces']['title']!r} -> {after['faces']['title']!r}",
            after["faces"]["title"] != before["faces"]["title"],
        )
        ok(
            f"C: ★ and its own sentence names the type it now claims — "
            f"{after['drawn']!r}",
            "Udp" in after["drawn"]["type"],
        )
        ok(
            f"C: ★★★★★ while the neighbour did NOT move, so this is per CARD "
            f"and not one global palette — {neighbour_before!r}",
            card(tints(app, surface), neighbour)["faces"]["title"]
            == neighbour_before,
        )

        banner("D — ★★★★★ what a person authored outranks the kind")
        app.invoke(f"{surface}/tint", f"{subject},#E01010")
        app.tick_ms(16)
        authored = card(tints(app, surface), subject)
        ok(
            f"D: ★★★★★ the authored colour is what the card wears — "
            f"{authored['faces']['title']!r}",
            authored["faces"]["title"] == "#E01010",
        )
        ok(
            "D: ★ and the kind's own sentence is UNCHANGED underneath it, so "
            "the ranking hid nothing",
            authored["drawn"] == after["drawn"],
        )
        # ★ Clearing hands the card back to its KIND, not to nothing — the half
        # a one-way assertion would miss entirely.
        app.invoke(f"{surface}/tint", f"{subject},none")
        app.tick_ms(16)
        cleared = card(tints(app, surface), subject)
        ok(
            f"D: ★★★★★ cleared, it falls back to what its KIND says rather "
            f"than to nothing — {cleared['faces']!r}",
            cleared["tint"] is None
            and cleared["faces"]["title"] == after["faces"]["title"],
        )

        banner("E — ★ one ranking feeds one derivation")
        ok(
            "E: ★ all four faces are present for whichever colour won",
            all(
                key in cleared["faces"]
                for key in ("title", "body", "comment", "title_text")
            ),
        )
        ok(
            f"E: ★ and the title's LETTERS are still chosen by contrast, never "
            f"authored — {cleared['faces']['title_text']!r}",
            cleared["faces"]["title_text"] in ("#000000", "#FFFFFF"),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1940 a card wears what it speaks", body)
