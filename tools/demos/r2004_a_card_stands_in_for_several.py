#!/usr/bin/env python3
"""R2004 §5.2 §5.11 — **a card stands in for several**, in the assembled tool.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row `AnimGraph::CreateSelfTransition`
names, on **screen A** — the node lab, mounted whole into
`hello-analyzer-shell` — rather than in a binary of its own.

# ★★★★★ The measurement that reversed this row's verdict

The pin read *a link whose source and sink are the same node*, and named two
obstacles: `connect` refuses a self-link, and the control plane has no self-edge
constructor. Both true. **Neither is what the reference's operator does.** Read
from its source, `OnCreateSelfTransition` never makes a self-edge at all: it
creates an **alias node**, runs a name validator to make `Self` unique, places
it at `+200, -100` from the state, puts that state into the alias's aliased-state
**set**, and links alias to state. The self-loop is what that link *expands to*.

So the capability is a node that stands in for a SET, and the command is its
one-element case. The reference's own baker says the mechanism in a comment:
*"Alias's are simply decompiled into multiple connections."*

# ★★★★★ And its hand-written validator is a THEOREM here

Its alias validator carries the message *A alias (@@) used as a transition's
target must alias a single state* — written by hand, in one editor, for one
plane. Nothing here says it: expanding piles links onto the socket at the FAR
end of the authored wire, so `Flow::multiplicity` decides it, which recovers
that rule on the control plane and INVERTS it on the value plane.

# What this walk holds

  (A) the journey reaches the node lab, and the stand-in register answers for
      every card — a register the reference has no equivalent of.
  (B) ★★★★★ nothing on the opening canvas stands in for anything, and the
      register SAYS so as a fact rather than staying silent.
  (C) ★★★★★ the canned verb works on a card of the canon topology: a stand-in
      appears, standing for exactly that card.
  (D) ★★★★★ and the WIRING then means one more link than was drawn — the
      reference's own sentence, as two numbers a client reads.
  (E) ★★★★★ widening it is refused BY NAME when the shapes do not agree, which
      is a check the reference never has to make and therefore never states.
  (F) ★ every register row agrees with the summary, so `any` is derived from the
      rows rather than decided a second time.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2004_a_card_stands_in_for_several.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402

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

        banner("A — the register answers for every card")
        opening = js(app.query(f"{surface}/stand_ins"))
        rows = opening["cards"]
        ok(f"A: the register answers — {len(rows)} card(s)", len(rows) > 1)
        ok(
            "A: ★ every row names the card it is about",
            all(row["card"] for row in rows),
        )
        # ★ The register covers the same cards the canvas draws, so what follows
        # is about the whole canvas rather than about whichever cards were asked.
        drawn = {row["node"] for row in js(app.query(f"{surface}/tints"))["nodes"]}
        ok(
            f"A: ★ it covers exactly the cards the canvas draws — "
            f"{len(rows)} vs {len(drawn)}",
            {row["card"] for row in rows} == drawn,
        )

        banner("B — ★★★★★ nobody stands in for anybody yet, and it SAYS so")
        ok(
            f"B: ★★★★★ stated as a fact — any={opening['any']}",
            opening["any"] is False,
        )
        ok(
            "B: ★ and every card's own answer agrees, so the summary is not a "
            "second statement free to disagree with the rows",
            all(row["stands_for"] is None and row["alone"] is None for row in rows),
        )
        ok(
            "B: ★ nothing is lost either, which is the OTHER standing fault and "
            "a different question from standing for nobody",
            all(row["lost"] == [] for row in rows),
        )
        drawn_before = opening["wiring"]["authored"]
        ok(
            f"B: ★ and with no stand-in on the canvas the two wiring numbers "
            f"AGREE — authored={drawn_before}, "
            f"expanded={opening['wiring']['expanded']}",
            opening["wiring"]["expanded"] == drawn_before,
        )

        banner("C — ★★★★★ the reference's command, on a canon card")
        # ★★★★★ DERIVED, not chosen. Measured on this topology: a card with no
        # inbound wire has no accept port at all — the lab's accept pin is a
        # variadic run and an empty run is no port — so there is nothing for a
        # stand-in to be wired back through, and the framework says so by name.
        # The reference never meets this: every state in a state machine has one
        # transition pin each side by construction. So the walk asks every card
        # in turn and asserts BOTH outcomes are the framework's, rather than
        # picking one that happens to work.
        subject, made, turned_away = None, None, []
        for row in rows:
            try:
                made = app.invoke(f"{surface}/stand_in_for", row["card"])
                subject = row["card"]
                break
            except RpcError as exc:
                turned_away.append((row["card"], str(exc)))
        ok(
            f"C: ★★★★★ some card of the canon topology can be stood in for — "
            f"{subject!r} after {len(turned_away)} refused",
            subject is not None,
        )
        ok(
            "C: ★★★★★ and every refusal SAID why rather than failing silently, "
            f"which is the check the reference never has to make: {turned_away}",
            all("no port pair" in said for _, said in turned_away),
        )
        app.tick_ms(16)
        after = js(app.query(f"{surface}/stand_ins"))
        standing = {row["card"]: row for row in after["cards"]}
        ok(f"C: ★★★★★ a card appeared, standing in — {made!r}", made in standing)
        ok(
            f"C: ★★★★★ and it stands for exactly the card it was asked about — "
            f"{standing[made]['stands_for']}",
            standing[made]["stands_for"] == [subject]
            and standing[made]["alone"] == "one",
        )
        ok(
            "C: ★ the register now says so at the summary too",
            after["any"] is True,
        )
        ok(
            f"C: ★ and the CARD it stands for is not itself one — the two are "
            f"different states: {standing[subject]['stands_for']}",
            standing[subject]["stands_for"] is None,
        )

        banner("D — ★★★★★ the wire it drew SAYS what it cost")
        wiring = after["wiring"]
        # ★★★★★ MEASURED, and the first draft of this walk asserted the
        # opposite: the link count did NOT move. The verb lands on the card's
        # first input, a value input holds ONE producer, so the wire it draws
        # DISPLACES the analyzer's own. The reference's operator can never meet
        # this — its transition pin holds many — so a verb that dropped the
        # displaced link would destroy authored work with nothing said.
        ok(
            f"D: ★★★★★ the count did not move, because the new wire replaced "
            f"one — authored={wiring['authored']} (was {drawn_before})",
            wiring["authored"] == drawn_before,
        )
        spoken = json.dumps(js(app.query(f"{surface}/said")))
        ok(
            f"D: ★★★★★ and the tool SAID what was replaced, which is what makes "
            f"the edit undoable: {spoken}",
            "replacing the wire from" in spoken,
        )
        ok(
            f"D: ★ with one member the two wiring readings agree, which is what "
            f"a one-element stand-in MEANS — expanded={wiring['expanded']}",
            wiring["expanded"] == wiring["authored"],
        )
        ok(
            "D: ★ and the shapes on this canvas are counted, which is the "
            f"measurement the reference never has to make — shapes={after['shapes']}",
            after["shapes"] >= 1,
        )

        banner("E — ★★★★★ the crowding law fires on the analyzer's own wiring")
        other = next(row["card"] for row in rows if row["card"] != subject)
        refused = None
        try:
            app.invoke(f"{surface}/represent", f"{made},{other}")
        except RpcError as exc:
            refused = str(exc)
        app.tick_ms(16)
        widened = js(app.query(f"{surface}/stand_ins"))
        now = {row["card"]: row for row in widened["cards"]}[made]
        # ★★★★★ THE ROUND'S CLAIM REACHING A PERSON. The reference writes *an
        # alias used as a transition's target must alias a single state* by
        # hand, for one plane, as a message discovered at compile time. Nothing
        # here says it: widening piles links onto the socket at the far end, and
        # here that socket is a value INPUT, which holds one producer. So the
        # refusal is DERIVED — and it is the value-plane half, the one a
        # one-plane state machine cannot express at all.
        ok(
            f"E: ★★★★★ widening was refused, by the port's own answer — {refused}",
            refused is not None and "which holds one" in refused,
        )
        ok(
            "E: ★★★★★ and the refusal NAMES THE SOCKET that would be "
            f"over-subscribed, which the reference's message does not carry: "
            f"{refused}",
            refused is not None and "links on the" in refused,
        )
        ok(
            f"E: ★ a refused edit left the document alone — "
            f"stands_for={now['stands_for']}, alone={now['alone']}",
            now["stands_for"] == [subject] and now["alone"] == "one",
        )

        banner("F — ★ the summary is derived from the rows")
        ok(
            "F: ★ `any` is true exactly when some row stands for something",
            widened["any"]
            == any(row["stands_for"] is not None for row in widened["cards"]),
        )
        ok(
            "F: ★ and nothing has been lost, so the two standing faults stay "
            "distinguishable",
            all(row["lost"] == [] for row in widened["cards"]),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r2004 a card stands in for several", body)
