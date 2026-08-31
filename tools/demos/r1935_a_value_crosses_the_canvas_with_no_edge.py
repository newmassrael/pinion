#!/usr/bin/env python3
"""R1935 §5.2 §5.11 — **a value crosses the canvas with no edge.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the FOUR reference-census rows name — the pair of
conversions between a plain bend and a named one, and the two directions of
reading a name — through the node lab as it is mounted in the shell.

# ★★★★★ Four rows, and reading all four is what showed they are not one

The census sentence for one of them was "the other direction of a named
reroute", and that is what hid the finding. Measured on the reference's four
operators:

  * **from a far end to the named endpoint** the answer is ONE node, so the
    operator moves the selection there and fits the view to it;
  * **from the endpoint to its far ends** the answer is MANY, so the operator
    *clears* the selection instead and hands the list to the search-results
    panel. Navigating is not a thing you can do to three nodes at once.

So the two directions do not differ only in which way they walk — they differ in
the SHAPE of the answer, and therefore in what an editor can do with it. The
other pair is a FAN-OUT and a FOLD: a bend with N outgoing wires becomes one
named endpoint and N far ends, and the fold accepts EITHER half as the thing you
started from.

# What the lab publishes for it

`name_bend` and `unname_bend` take one card each, because the fan-out has
nothing else to choose. `names` is the register, and it carries both shapes:
`endpoints` lists what each name reaches, `far` says which name each far end
shows and which endpoint it belongs to, and `dangling` is the state the
reference leaves to a predicate nobody is obliged to call.

# What this walk holds

  (A) the journey reaches the node lab, so what follows is about the ASSEMBLED
      tool, and NOTHING on the opening canvas answers to a name.
  (B) ★★★★★ a bend with three wires leaving it becomes ONE name reached by
      THREE far ends — the fan-out, which a rename would fail.
  (C) ★★★★★ THE VALUE CROSSES WITH NO EDGE: not one wire joins the endpoint to
      any far end, and the wire count went UP by none.
  (D) ★★★★★ the register answers the two directions in two SHAPES — a list from
      the endpoint, one endpoint from each far end.
  (E) ★★★★★ the fold accepts a FAR END, folds every card of the name back into
      one bend, and the tool still says the same thing about itself.
  (F) the refusal: a card that is neither half is turned away with the reason
      rather than doing nothing.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1935_a_value_crosses_the_canvas_with_no_edge.py
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

#: The card three wires leave, named rather than discovered — the same fan-out
#: R1934 used, for the same reason: a walk that hunted for one would quietly
#: assert about whichever it found.
FAN_OUT = "R-01"

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


def leaving(rows: list[dict], card: str) -> list[dict]:
    return [row for row in rows if row["from"] == card]


def passing(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/passing"))["through"]


def names(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/names"))


def verdict(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/verdict"))


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

        banner("A — nothing on the opening canvas answers to a name")
        opening = names(app, surface)
        ok(
            f"A: the register answers, and every part of it is empty — {opening}",
            opening["endpoints"] == []
            and opening["far"] == []
            and opening["dangling"] == [],
        )
        before_links = links(app, surface)
        fan = leaving(before_links, FAN_OUT)
        ok(
            f"A: ★ {FAN_OUT} fans out to {[row['to'] for row in fan]} — a "
            "fan-out with one wire could not be told from a rename",
            len(fan) >= 3,
        )
        was = verdict(app, surface)
        ok(f"A: and the tool says what it says about itself — {was}", "sentence" in was)

        banner("B — ★★★★★ a bend becomes ONE name reached by THREE far ends")
        # The bend first: this round's capability is about a bend that already
        # carries a fan-out, which is R1934's verb.
        app.invoke(f"{surface}/insert_reroute", f"{FAN_OUT},dial")
        app.tick_ms(16)
        bends = passing(app, surface)
        ok(f"B: one bend is on the canvas — {bends}", len(bends) == 1)
        bend_card = bends[0]["card"]
        wires_before = len(links(app, surface))

        said = app.invoke(f"{surface}/name_bend", bend_card)
        app.tick_ms(16)
        after = names(app, surface)
        ok(
            f"B: the verb says what it did — {said!r}",
            "far end" in str(said),
        )
        ok(
            f"B: ★ exactly one endpoint — {after['endpoints']}",
            len(after["endpoints"]) == 1,
        )
        endpoint = after["endpoints"][0]
        ok(
            f"B: ★★★★★ and it is reached by {len(endpoint['reaches'])} far "
            f"end(s), one per wire that was leaving the bend — {endpoint}",
            len(endpoint["reaches"]) == len(fan),
        )
        ok(
            f"B: the bend itself is gone from the passing register — "
            f"{passing(app, surface)}",
            all(row["card"] != bend_card for row in passing(app, surface)),
        )

        banner("C — ★★★★★ THE VALUE CROSSES WITH NO EDGE")
        joined = [
            row
            for row in links(app, surface)
            if row["from"] == endpoint["card"] and row["to"] in endpoint["reaches"]
        ]
        ok(
            f"C: ★★★★★ not one wire joins the endpoint to a far end — {joined}",
            joined == [],
        )
        ok(
            f"C: ★ and the canvas has no MORE wires than before the name "
            f"({wires_before} then {len(links(app, surface))}) — the far ends "
            "were reached by name, not by adding edges",
            len(links(app, surface)) == wires_before,
        )

        banner("D — ★★★★★ the two directions answer two SHAPES")
        far = after["far"]
        ok(
            f"D: ★ every far end names ONE endpoint, and it is that one — {far}",
            len(far) == len(endpoint["reaches"])
            and all(row["endpoint"] == endpoint["card"] for row in far),
        )
        ok(
            f"D: ★ and each shows the endpoint's own name rather than its own — "
            f"{[row['shows'] for row in far]}",
            len({row["shows"] for row in far}) == 1,
        )
        ok(
            f"D: ★★★★★ MANY from the endpoint, ONE from each far end — which is "
            "why an editor navigates on the second and lists on the first",
            len(endpoint["reaches"]) > 1 and all("endpoint" in row for row in far),
        )
        ok(
            f"D: nothing is dangling — {after['dangling']}",
            after["dangling"] == [],
        )

        banner("E — ★★★★★ the fold accepts a FAR END, and the tool is unchanged")
        a_far_end = far[0]["card"]
        folded = app.invoke(f"{surface}/unname_bend", a_far_end)
        app.tick_ms(16)
        ok(
            f"E: ★ named from a far end, and it folded the whole name — {folded!r}",
            "fold back into the bend" in str(folded),
        )
        ok(
            f"E: ★ nothing answers to a name any more — {names(app, surface)}",
            names(app, surface)["endpoints"] == []
            and names(app, surface)["far"] == [],
        )
        ok(
            f"E: ★ and one bend is back on the canvas — {passing(app, surface)}",
            len(passing(app, surface)) == 1,
        )
        ok(
            "E: ★★★★★ AND THE TOOL STILL SAYS THE SAME THING ABOUT ITSELF — "
            "naming a bend is not an edit to the pipeline",
            verdict(app, surface) == was,
        )

        banner("F — the refusal says which way the conversion goes")
        why = refusal(app, f"{surface}/unname_bend", FAN_OUT)
        ok(
            f"F: ★ a card that is neither half is turned away with the reason — {why!r}",
            why is not None and "named endpoint" in why,
        )
        ok(
            "F: and the canvas is unchanged by the refusal",
            names(app, surface)["endpoints"] == [],
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1935 a value crosses the canvas with no edge", body)
