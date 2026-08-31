#!/usr/bin/env python3
"""R1925 §5.11 §2 #7 — **the assembled tool gathers a definition's face into
named, collapsible sections, and says why it cannot switch one.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability three reference-census rows name — the DCC's *new
panel toggle*, *make panel toggle* and *unlink panel toggle* — through the node
lab as it is mounted in the shell.

# ★★★★★ What R1925 measured before it built anything

The census carried all three under the sentence *grouping a definition's ports
into collapsible sections*. Read at the reference's own operator file, that
sentence covers none of them: the three are **the switch** — one boolean input
inside a panel stands for the whole panel — and grouping has no row of its own
in that list at all. That is the THIRD round in a row whose covering sentence
was false when measured clause by clause (R1923, R1924).

The same reading found a second thing: `interface_item_new` was pinned `have`
against `Document::expose`, and its own `item_type` enum has three members —
INPUT, OUTPUT and **PANEL**. No `expose` can make a panel, so a third of that
operator was carried by a proof that never reached it.

# ★ Why this screen publishes rather than draws

A definition's face has no pixels on the behaviour canon — the reference
mock-up has node collapse and palette collapse, and no interface panel
anywhere. So this round drew nothing new: it published the register, which is
what an AI-first framework owes a capability its screen has no gesture for
(§2 #2, §2 #7). What the walk asserts is that the published register is REAL —
state goes in, reads back, and comes out again.

# What this walk holds

  (A) the shell publishes this graph's face: its sections, its port count, and
      whether this application has a switchable type at all.
  (B) ★★★★★ a section can be ADDED through the wire and reads back with its
      header — the reference's `item_type='PANEL'` clause, on the screen.
  (C) ★★★★★ *collapsible* is a state, not an adjective: folding one reads back
      folded, and its neighbour does not move.
  (D) ★★★★★ the switch question is answered by the FRAMEWORK, not re-derived
      here, and the refusal names the application's own missing declaration
      rather than a framework limit.
  (E) the screen refuses what it cannot address — a second section of the same
      header, and a command word it does not have — with a sentence saying what
      WAS acceptable.
  (F) and a removal takes the section away and leaves the other one standing.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1925_a_face_gathers_into_sections.py
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


def face(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/sections"))


def refused(app: RpcSubprocess, surface: str, args: str) -> str:
    """Drive the section verb and hand back the refusal's sentence."""
    try:
        app.invoke(f"{surface}/section", args)
    except Exception as why:  # noqa: BLE001 — the sentence is the subject
        return str(why)
    raise AssertionError(f"{args!r} was accepted and should not have been")


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

        banner("A — the shell publishes this graph's own face")
        opening = face(app, surface)
        ok(
            f"A: the register is there and this face opens with no sections — "
            f"{opening['sections']!r}",
            opening["sections"] == [],
        )
        ok(
            "A: ★ and it says whether this application has a switchable type at "
            f"all — switchable={opening['switchable']!r}",
            opening["switchable"] is False,
        )

        banner("B — ★★★★★ a section is added through the wire")
        app.invoke(f"{surface}/section", "add,Transport")
        app.tick_ms(16)
        app.invoke(f"{surface}/section", "add,Traffic")
        app.tick_ms(16)
        after = face(app, surface)
        headers = [held["name"] for held in after["sections"]]
        ok(f"B: ★ both headers read back, in the order they were made — {headers}", headers == ["Transport", "Traffic"])
        ok(
            "B: ★ and each arrives empty and open, which is what an unarranged "
            "section is",
            all(
                held["members"] == [] and held["folded"] is False
                for held in after["sections"]
            ),
        )
        ok(
            "B: ★★★★★ and none of them has a switch, so what follows is about a "
            "capability this screen genuinely does not have rather than one it "
            "forgot to use",
            all(held["switch"] is None for held in after["sections"]),
        )

        banner("C — ★★★★★ collapsible is a STATE")
        app.invoke(f"{surface}/section", "fold,Transport,on")
        app.tick_ms(16)
        folded = {held["name"]: held["folded"] for held in face(app, surface)["sections"]}
        ok(f"C: ★ the one that was folded reads back folded — {folded}", folded["Transport"] is True)
        ok(
            "C: ★★★★★ and its neighbour did not move — a fold that folded the "
            "whole face would satisfy the line above on its own",
            folded["Traffic"] is False,
        )
        app.invoke(f"{surface}/section", "fold,Transport,off")
        app.tick_ms(16)
        ok(
            "C: and it opens again, so this is a state and not a one-way door",
            face(app, surface)["sections"][0]["folded"] is False,
        )

        banner("D — ★★★★★ the switch question is the FRAMEWORK's answer")
        asked = face(app, surface)["because"]
        ok(
            f"D: ★ the screen publishes a refusal rather than a silence — {asked!r}",
            asked is not None,
        )
        ok(
            f"D: ★★★★★ and it is the APPLICATION's own gap, not a framework "
            f"limit — {asked['word']!r}",
            asked["word"] == "no-switch-type",
        )
        ok(
            "D: ★★★★★ the sentence says which declaration is missing, so a "
            "reader knows what would have to change — "
            f"{asked['sentence']!r}",
            "two-state" in asked["sentence"],
        )
        ok(
            "D: ★ and the two halves agree: a face that says it has no "
            "switchable type must be the one refusing for that reason",
            (asked["word"] == "no-switch-type")
            == (face(app, surface)["switchable"] is False),
        )

        banner("E — the screen refuses what it cannot address")
        why = refused(app, surface, "add,Transport")
        ok(
            f"E: ★ a second section of the same header is refused, because a "
            f"header is how the wire names one — {why!r}",
            "already has a section" in why,
        )
        why = refused(app, surface, "wobble,Transport")
        ok(
            f"E: ★★ and an unknown command names the ones there ARE, so a caller "
            f"is not left guessing — {why!r}",
            "add" in why and "fold" in why and "remove" in why,
        )
        why = refused(app, surface, "fold,Nowhere,on")
        ok(f"E: and a section this face does not have is named back — {why!r}", "Nowhere" in why)

        banner("F — a removal takes one away and leaves the other")
        app.invoke(f"{surface}/section", "remove,Transport")
        app.tick_ms(16)
        left = [held["name"] for held in face(app, surface)["sections"]]
        ok(f"F: ★ one section is gone and the other stands — {left}", left == ["Traffic"])

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1925_a_face_gathers_into_sections", body))
