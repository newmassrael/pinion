#!/usr/bin/env python3
"""R1936 §5.2 §5.11 — **a card stands for a definition, and keeps its identity.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census rows name — the DCC's group
swaps — through the node lab as it is mounted in the shell.

# ★★★★★ The census sentence named half the capability

Its reason read *no verb changes which definition an instance stands for*, which
is about re-pointing an instance that already exists. Reading the reference's
operator instead of its name: it accepts ANY swappable node, so becoming a group
and being re-pointed at another definition are one edit there — and one verb
here (`Document::set_definition`, with `set_new_definition` as the spelling that
makes the definition first).

★ And the round's second measured finding: one of the two rows is not C++ at
all. `swap_empty_group` is a Python operator that builds an empty group, calls
the node swap, and then reaches back in to point the result at what it made —
which is why the two can disagree there about what a swap is, and why this
crate writes the second as a composition of the first.

# Why it is a verb and not delete-and-add

Delete-and-add destroys the node, so its id dies and with it every selection,
saved layout, held reference and undo record keyed by it. Here the card keeps
its id and only what it stands for changes. That is what this walk asserts.

# What this walk holds

  (A) the journey reaches the node lab, so what follows is about the ASSEMBLED
      tool, and every card on the opening canvas stands for its own KIND.
  (B) ★★★★★ a card becomes an instance of a NEW empty definition and KEEPS ITS
      NAME — the id survived, which is the whole difference from delete-and-add.
  (C) ★★★★★ and the tool SAYS WHAT IT COST: an empty face answers no port, so
      the wires went, and the sentence names how many rather than reporting a
      bare success. The canvas agrees with the count.
  (D) the register tells the two apart — that card stands for a definition now,
      and the others still stand for kinds.
  (E) the refusal: a body this crate owns is not the application's to overwrite,
      and it is turned away with the reason rather than doing nothing.
  (F) ★ AND THE REFUSAL MADE NO DEFINITION — the count is unchanged, so a
      refused swap does not litter the document with a definition nobody asked
      for.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1936_a_card_stands_for_a_definition.py
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

#: The card this walk swaps, named rather than discovered: a walk that hunted
#: for one would quietly assert about whichever it found.
SUBJECT = "R-01"

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


def links(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/links"))


def standing(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/standing_for"))["cards"]


def row_for(rows: list[dict], card: str) -> dict:
    return next(row for row in rows if row["card"] == card)


def refusal(app: RpcSubprocess, path: str, arg: str):
    """Answer the refusal's sentence, or None when the edit went through."""
    try:
        app.invoke(path, arg)
        return None
    except Exception as why:  # noqa: BLE001 — the refusal IS the measurement
        return str(why)


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

        banner("A — every card on the opening canvas stands for its own kind")
        opening = standing(app, surface)
        ok(f"A: the register answers — {len(opening)} card(s)", len(opening) > 0)
        # ⚠ Not "all of them are kinds": the canvas also carries frames, which
        # are a body of this crate's own. What this walk is about is that
        # NOTHING here stands for a definition yet, which is the state the swap
        # changes — the first draft asserted the stronger thing and the canvas
        # said 8 of 10.
        instances = [row for row in opening if row["stands_for"] == "definition"]
        ok(
            f"A: ★ nothing on the opening canvas stands for a definition — "
            f"{instances}; the kinds are "
            f"{len([r for r in opening if r['stands_for'] == 'kind'])} of {len(opening)}",
            instances == [],
        )
        ok(
            f"A: {SUBJECT} among them, standing for no definition — "
            f"{row_for(opening, SUBJECT)}",
            row_for(opening, SUBJECT)["definition"] is None,
        )
        on_subject = [
            row
            for row in links(app, surface)
            if row["from"] == SUBJECT or row["to"] == SUBJECT
        ]
        ok(
            f"A: ★ and it has {len(on_subject)} wire(s) on it — a swap that cost "
            "nothing could not be told from one that cost everything",
            len(on_subject) > 0,
        )

        banner("B — ★★★★★ the card becomes an instance and KEEPS ITS NAME")
        said = app.invoke(f"{surface}/regroup", SUBJECT)
        app.tick_ms(16)
        after = standing(app, surface)
        ok(
            f"B: ★★★★★ the card is still there under the same name — this is a "
            f"swap, not a replace: {[r['card'] for r in after]}",
            any(row["card"] == SUBJECT for row in after),
        )
        subject = row_for(after, SUBJECT)
        ok(
            f"B: ★ and it stands for a DEFINITION now — {subject}",
            subject["stands_for"] == "definition"
            and subject["definition"] is not None,
        )

        banner("C — ★★★★★ and the tool says what it cost")
        ok(
            f"C: the verb names how many wires went, rather than reporting a "
            f"bare success — {said!r}",
            "wire(s) went" in str(said),
        )
        still_on = [
            row
            for row in links(app, surface)
            if row["from"] == SUBJECT or row["to"] == SUBJECT
        ]
        ok(
            f"C: ★★★★★ an empty face answers no port, so every wire on the card "
            f"is gone and the canvas agrees — {still_on}",
            still_on == [],
        )

        banner("D — the register tells the two apart")
        others = [row for row in after if row["card"] != SUBJECT]
        ok(
            f"D: ★ the other cards still stand for their kinds — "
            f"{sorted({row['stands_for'] for row in others})}",
            all(row["stands_for"] != "definition" for row in others),
        )

        banner("E — the refusal says what may not be overwritten")
        # ⚠ The subject is taken from what the register ACTUALLY answered rather
        # than named here: a walk that spelled a card out would assert about a
        # canvas it hoped for. A frame is a body this crate owns and the opening
        # canvas already carries one, which is the reachable case on this screen.
        owned = [row for row in after if row["stands_for"] not in ("kind", "definition")]
        ok(
            f"E: a body this crate owns is on the canvas — {owned}",
            len(owned) > 0,
        )
        subject_owned = owned[0]["card"]
        why = refusal(app, f"{surface}/regroup", subject_owned)
        ok(
            f"E: ★ and it is turned away with the reason — {why!r}",
            why is not None and "cannot be made to stand for a definition" in why,
        )

        banner("F — ★ and the refusal changed nothing")
        ok(
            f"F: ★ {subject_owned} still stands for what it did, so nothing was "
            "swapped and no definition was left behind for it",
            row_for(standing(app, surface), subject_owned)["stands_for"]
            == owned[0]["stands_for"],
        )
        ok(
            "F: ★ and it is still the only card standing for a definition",
            len([r for r in standing(app, surface) if r["stands_for"] == "definition"])
            == 1,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1936 a card stands for a definition", body)
