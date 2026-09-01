#!/usr/bin/env python3
"""R1921 §5.11 §5.12 — **a card wears a colour a person chose, and its letters
stay readable on it.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability five reference-census rows name and none of them
covered — the DCC's copy-colour and the engine's four node-face colours —
through the node lab as it is mounted in the shell.

# ★★★★★ The property, and why it has to be checked ON THE FRAME

The engine asks a node four INDEPENDENT questions for its four face colours,
and two of them are the title's fill and the title's LETTERS. Nothing there
relates the two answers, so a subclass can darken one without the other and
produce a title nobody can read — and no check in that model can notice,
because each virtual is correct on its own.

Here one colour is authored and the letters are CHOSEN by contrast against the
fill they will sit on. That is a claim about what gets PAINTED, so a crate test
alone cannot finish it: the crate can prove `Faces` picks contrasting ink, and
still leave the screen painting that ink onto a different face. (The first
draft of this round did exactly that — it filled the card with `body` while
lettering it for `title`.) So this walk reads BOTH the fill and the letters off
the same frame and holds the contrast between the two it actually finds.

# What this walk holds

  (A) with nothing coloured, every card carries no colour and the row says so.
  (B) a colour given over the wire CHANGES THE FILL ON THE FRAME.
  (C) ★ the derived faces are published, and `title` is the authored colour
      while `body` and `comment` are progressively further back.
  (D) ★★★★★ THE LAW ON THE FRAME: for a LIGHT colour and for a DARK one, the
      letters differ — and each time they contrast with the fill that was
      really painted, read from the same snapshot.
  (E) a malformed colour is REFUSED and the card keeps what it had.
  (F) `none` takes the colour away, and the frame returns exactly to (A).

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1921_a_card_wears_a_colour_a_person_chose.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    find_by_tag,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

# A light colour and a dark one, chosen to land on OPPOSITE sides of the
# contrast decision so (D) is a real comparison rather than one case twice.
#
# ⚠ R1943 — UPPERCASE, and that is a fact about the wire rather than a style:
# R1940 found this screen writing one colour in two cases (its card register
# lowercase, its ink register uppercase through a shared helper) so a client
# comparing a card with the pin it takes its colour from would have found them
# unequal, and put both through the helper. This walk had been reading the
# lowercase half.
LIGHT = "#F0E68C"
DARK = "#2A2A55"

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


def tints(app: RpcSubprocess, surface: str) -> dict:
    return {row["node"]: row for row in js(app.query(f"{surface}/tints"))["nodes"]}


def luminance(colour: dict) -> float:
    """The same weighting the crate uses, so this walk and the screen agree on
    what "contrast" means rather than each having an opinion."""
    return 0.213 * colour["r"] + 0.715 * colour["g"] + 0.072 * colour["b"]


def card_paint(app: RpcSubprocess, name: str) -> tuple[dict, dict]:
    """The card's FILL and the colour of its identifier's letters, from ONE
    snapshot — the pair the contrast property is about.

    ⚠ A box and a run do not carry their colour in the same field: a container
    is `style.fill`, a text run is `style.fg_color`. Reading both off one
    snapshot is what makes the gap below a fact about one frame.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    box = find_by_tag(snap, f"lab.node.{name}") or {}
    label = find_by_tag(snap, f"lab.node.{name}.id") or {}
    return box.get("style", {}).get("fill"), label.get("style", {}).get("fg_color")


def cards(app: RpcSubprocess) -> list[str]:
    return sorted(
        tag.removeprefix("lab.node.")
        for tag in abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
        if tag.startswith("lab.node.") and tag.count(".") == 2
    )


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
        drawn = cards(app)
        ok(f"the canvas draws cards to colour — {len(drawn)}", len(drawn) >= 2)
        subject = drawn[0]

        banner("A — nothing is coloured")
        rows = tints(app, surface)
        ok(
            f"A: ★ every drawn card has a row — {sorted(set(drawn) - set(rows))} missing",
            set(drawn) <= set(rows),
        )
        ok(
            "A: and none of them carries a colour",
            all(row["tint"] is None for row in rows.values()),
        )
        # ⚠ R1943 — this assertion USED to read "nor any derived faces", on the
        # ground that a card with no authored colour derives nothing. R1940
        # changed what that means and this walk was not re-run: a kind now says
        # what its node is drawn as, so an uncoloured card DOES have faces —
        # they come from the taxonomy rather than from a default nobody chose.
        # The property this assertion exists to hold is unchanged, and is now
        # stated as what it always meant: no face is invented HERE.
        ok(
            "A: ★ and any faces they wear come from the KIND, never from an "
            "authored colour, because there is none to derive from",
            all(
                row["faces"] is None or row["drawn"]["says"] != "unstated"
                for row in rows.values()
            ),
        )
        bare_fill, bare_ink = card_paint(app, subject)
        ok(f"A: the card is painted in its kind's surface — {bare_fill}", bare_fill)

        banner("B — a colour given over the wire lands on the frame")
        ok(
            f"B: the verb answers what the card now carries — {LIGHT}",
            app.invoke(f"{surface}/tint", f"{subject},{LIGHT}") == LIGHT,
        )
        app.tick_ms(16)
        lit_fill, lit_ink = card_paint(app, subject)
        ok(
            f"B: ★★★★★ and the FILL CHANGED — {bare_fill} then {lit_fill}",
            lit_fill != bare_fill,
        )
        ok(
            "B: ★ and only that card changed",
            all(
                tints(app, surface)[other]["tint"] is None
                for other in drawn
                if other != subject
            ),
        )

        banner("C — the faces are published, and they are a progression")
        row = tints(app, surface)[subject]
        ok(f"C: the authored colour is carried back — {row['tint']}", row["tint"] == LIGHT)
        faces = row["faces"]
        ok(f"C: and its faces are published — {faces}", faces is not None)
        ok("C: ★ the title face IS the authored colour", faces["title"] == LIGHT)
        as_rgb = lambda hexed: {  # noqa: E731 - a local, used twice
            "r": int(hexed[1:3], 16),
            "g": int(hexed[3:5], 16),
            "b": int(hexed[5:7], 16),
        }
        ok(
            f"C: ★ body sits behind title, comment behind body — "
            f"{faces['title']} {faces['body']} {faces['comment']}",
            luminance(as_rgb(faces["title"]))
            > luminance(as_rgb(faces["body"]))
            > luminance(as_rgb(faces["comment"])),
        )

        banner("D — ★★★★★ THE LAW ON THE FRAME, on both sides of the decision")
        # ★★★★★ FIRST: the letters were chosen for contrast against a NAMED
        # face, so the face that is actually painted has to be that one. This
        # assertion exists because the contrast gap below did NOT catch getting
        # it wrong: with the card filled from `body` while lettered for
        # `title`, the measured gap was 101 against a floor of 100 — the defect
        # sat one point inside the tolerance. R1862's lesson, met again: a
        # tolerance the size of the defect cannot see the defect, and the
        # repair is to assert the RELATION rather than to move the number.
        ok(
            f"D: ★★★★★ the face painted IS the face the letters were chosen "
            f"against — fill {lit_fill}, title {faces['title']}",
            lit_fill == as_rgb(faces["title"]) | {"a": 255},
        )
        light_gap = abs(luminance(lit_fill) - luminance(lit_ink))
        ok(
            f"D: on a LIGHT card the letters contrast with the fill REALLY "
            f"PAINTED — fill {lit_fill}, ink {lit_ink}, gap {light_gap:.0f}",
            light_gap >= 100,
        )
        app.invoke(f"{surface}/tint", f"{subject},{DARK}")
        app.tick_ms(16)
        dark_fill, dark_ink = card_paint(app, subject)
        dark_gap = abs(luminance(dark_fill) - luminance(dark_ink))
        ok(
            f"D: and on a DARK one — fill {dark_fill}, ink {dark_ink}, "
            f"gap {dark_gap:.0f}",
            dark_gap >= 100,
        )
        # ★★★★★ The two cases must actually DIFFER, or this section passed
        # twice on one case and proved nothing about the choice being made.
        ok(
            f"D: ★★★★★ and the two inks are DIFFERENT, so the contrast is a "
            f"CHOICE and not a constant — {lit_ink} against {dark_ink}",
            lit_ink != dark_ink,
        )

        banner("E — a malformed colour is refused, and nothing changes")
        held = tints(app, surface)[subject]["tint"]
        try:
            app.invoke(f"{surface}/tint", f"{subject},bright-ish")
            refused = False
            said = ""
        except Exception as why:  # noqa: BLE001 - the refusal is the subject
            refused = True
            said = str(why)
        ok(f"E: ★ it is refused — {said[:80]}", refused)
        ok("E: and the card keeps what it had", tints(app, surface)[subject]["tint"] == held)

        banner("F — `none` takes it away and the frame returns")
        ok(
            "F: the verb answers `none`",
            app.invoke(f"{surface}/tint", f"{subject},none") == "none",
        )
        app.tick_ms(16)
        back = tints(app, surface)[subject]
        ok("F: the row carries no colour", back["tint"] is None)
        # ⚠ R1943 — this used to read "and no faces either". R1940 made a kind
        # able to say what its node is drawn as, so clearing an authored colour
        # hands the card back to its KIND rather than to nothing — which is what
        # this screen's own success message ("back to its kind's colour") had
        # been claiming since R1921 while there was no such colour to go back
        # to. The property that still matters is that the AUTHORED colour is
        # gone and what remains is not it.
        ok(
            f"F: ★ and what it wears now is its kind's, not the colour that was "
            f"taken away — {back['faces']}",
            back["faces"] is None or back["faces"]["title"] != LIGHT,
        )
        gone_fill, gone_ink = card_paint(app, subject)
        ok(
            f"F: ★★★★★ and the frame is EXACTLY as it was before any colour — "
            f"{gone_fill} / {gone_ink}",
            (gone_fill, gone_ink) == (bare_fill, bare_ink),
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1921_a_card_wears_a_colour_a_person_chose", body))
