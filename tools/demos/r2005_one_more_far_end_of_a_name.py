#!/usr/bin/env python3
"""R2005 §5.2 §5.11 — **one more far end of a name**, in the assembled tool.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row
`MaterialEditor::CreateRerouteUsageFromDeclaration` names, on **screen A** — the
node lab, mounted whole into `hello-analyzer-shell`.

It is the FIFTH operator over R1935's named pair, and the one that GROWS the
far-end set where the other four read the two directions and convert both ways.
The model was already here; what was absent was the verb.

# ★★★★★ The three measurements that decided what was built

Read from `FMaterialEditor::OnCreateRerouteUsageFromDeclaration`:

1. **It has no activation condition.** The command is bound TWICE as a bare
   execute action with no can-execute predicate, and the only thing standing
   between it and a node it does nothing for is an `IsA(...Declaration)` test in
   the context-menu builder. So *may I* is a question that exists only where the
   menu is drawn.
2. **It stacks cards on one point.** The new usage goes to
   `NodePosX + 150, NodePosY` unconditionally, so a second call lands it exactly
   on the first — and nothing reports it.
3. **It clears the selection and selects nothing**, so a person is left with a
   new card and no indication which one it is.

# What this walk holds

  (A) the journey reaches the node lab, and the name register answers.
  (B) ★ the canon topology has no name yet, and the register SAYS so — so what
      follows is a name this walk made, not one it found.
  (C) a bend goes on a pin's outgoing wires and is given a name (R1934/R1935),
      which is what gives this round something to grow.
  (D) ★★★★★ one more far end appears, and the register shows the endpoint
      reaching one MORE card than before.
  (E) ★★★★★ a SECOND one does not land on the first — the walk asks the tool
      what it did, and the sentence names what it stepped past.
  (F) ★★★★★ asked of the wrong half, the tool REFUSES and the refusal carries
      the endpoint to ask instead — a question the reference does not have.
  (G) ★ nothing dangles and no wire was drawn, so the far ends reach the name
      the way the pair exists to.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2005_one_more_far_end_of_a_name.py
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

        banner("A — the name register answers")
        opening = js(app.query(f"{surface}/names"))
        ok(
            f"A: the register has both halves and the dangling list — "
            f"{sorted(opening)}",
            {"endpoints", "far", "dangling"} <= set(opening),
        )

        banner("B — ★ the canon topology carries no name yet")
        ok(
            f"B: ★ no endpoint, so what follows is a name THIS WALK made — "
            f"endpoints={opening['endpoints']}",
            opening["endpoints"] == [],
        )
        ok("B: ★ and nothing dangles", opening["dangling"] == [])

        banner("C — a bend is put on a wire and given a name")
        # ★ Derived, not chosen: the first card whose dial pin a bend will take.
        # A bend goes on the wires LEAVING a pin, so a card with nothing leaving
        # it is refused by name — which is R1934's own rule and not this
        # round's, so the walk takes the first that works.
        cards = [row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]]
        made, turned_away = False, []
        for card in cards:
            try:
                app.invoke(f"{surface}/insert_reroute", f"{card},dial")
                made = True
                break
            except RpcError as exc:
                turned_away.append((card, str(exc)))
        ok(
            f"C: a bend went on some card's outgoing wires — after "
            f"{len(turned_away)} refused",
            made,
        )
        app.tick_ms(16)
        # ★ The bend's own ADDRESS comes from the register that publishes what a
        # wire passes through, not from parsing the sentence the verb answered:
        # a sentence is for a person and an address is for a client, and R1935's
        # own finding is that the two directions of one fact differ in shape.
        through = js(app.query(f"{surface}/passing"))["through"]
        ok(f"C: ★ the canvas publishes what a wire passes through — {through}", through)
        bend = through[0]["card"]
        app.invoke(f"{surface}/name_bend", bend)
        app.tick_ms(16)
        named = js(app.query(f"{surface}/names"))
        ok(
            f"C: ★ the bend is now a NAME — endpoints={named['endpoints']}",
            len(named["endpoints"]) == 1,
        )
        endpoint = named["endpoints"][0]["card"]
        before = list(named["endpoints"][0]["reaches"])

        banner("D — ★★★★★ one more far end of that name")
        first = app.invoke(f"{surface}/echo_name", endpoint)
        app.tick_ms(16)
        grown = js(app.query(f"{surface}/names"))
        reaches = grown["endpoints"][0]["reaches"]
        ok(
            f"D: ★★★★★ the endpoint reaches one MORE card — {len(before)} -> "
            f"{len(reaches)}",
            len(reaches) == len(before) + 1,
        )
        ok(
            f"D: ★ and the new card is the one the tool named — {first!r}",
            first in reaches,
        )
        ok(
            "D: ★ it shows the endpoint's name and points back at it, which is "
            "what a far end IS",
            any(
                row["card"] == first and row["endpoint"] == endpoint
                for row in grown["far"]
            ),
        )

        banner("E — ★★★★★ a second one does not land on the first")
        second = app.invoke(f"{surface}/echo_name", endpoint)
        app.tick_ms(16)
        spoken = json.dumps(js(app.query(f"{surface}/said")))
        ok(
            f"E: ★★★★★ the tool says it went BELOW the one already there, "
            f"which the reference's own command cannot say because its position "
            f"does not depend on what is drawn: {spoken}",
            "below the" in spoken,
        )
        twice = js(app.query(f"{surface}/names"))
        ok(
            f"E: ★ and both are far ends of the one name — "
            f"{twice['endpoints'][0]['reaches']}",
            first in twice["endpoints"][0]["reaches"]
            and second in twice["endpoints"][0]["reaches"],
        )

        banner("F — ★★★★★ asked of the wrong half, it refuses WITH the repair")
        refused = None
        try:
            app.invoke(f"{surface}/echo_name", first)
        except RpcError as exc:
            refused = str(exc)
        ok(
            f"F: ★★★★★ a far end is refused — {refused}",
            refused is not None and "shows a name" in refused,
        )
        ok(
            f"F: ★★★★★ and the refusal NAMES THE ENDPOINT to ask instead, which "
            f"is a repair rather than a no. The reference has no such question "
            f"at all — its command carries no can-execute predicate: {refused}",
            refused is not None and "ask node" in refused,
        )
        after = js(app.query(f"{surface}/names"))
        ok(
            "F: ★ and the refused call changed nothing",
            after["endpoints"][0]["reaches"] == twice["endpoints"][0]["reaches"],
        )

        banner("G — ★ the name is reached over no wire, and nothing dangles")
        ok(
            f"G: ★ nothing dangles — {after['dangling']}",
            after["dangling"] == [],
        )
        ok(
            "G: ★ every far end of this name points back at it, so the register "
            "is not two statements free to disagree",
            all(
                row["endpoint"] == endpoint
                for row in after["far"]
                if row["card"] in after["endpoints"][0]["reaches"]
            ),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r2005 one more far end of a name", body)
