#!/usr/bin/env python3
"""R1978 §5.2 §5.21 — **five conditions a saved graph can be in, and five
answers a person gets.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the open path of the node lab — the screen the reference-census row
for saving and restoring names — along every condition `Archive::read` can end
in, reached through the assembled shell rather than through a screen binary of
its own.

# ★★★★★ What R1978 changed, and what it did NOT

The milestone this round was given said to *put a `refusal()` accessor on
`Opening` so the split between "unreadable" and "unsound" reaches the crate
surface*. Measured first, as standing rule (4) requires: **that accessor has
existed since R1689**, added by the commit that created the type. The
prescription was already satisfied and the round's premise was wrong.

What was NOT satisfied is the half the prescription was aiming at. The screen
did not ask the crate which condition it had — it worked the split out for
itself, from the *absence* of violations plus a fall-through to
`Opening::reason`. And so did the sibling node editor, differently. Two callers,
two hand-derivations of one rule, and they disagreed: for a refusal that also
carried violations the lab's answer was *openable* and the crate's was
*refused*. That state cannot be produced by `read`, which is why the
disagreement survived — nobody can test what nobody can reach.

R1978 publishes the three-way as one value (`Condition`), derives every other
reader of an `Opening` from it, and makes the two screens *state* their policy
instead of re-deriving it. The lab opens an unsound document and names its
faults; the editor refuses one. Both answers are defensible and they are now
answers rather than accidents of which accessor was reached for.

# What this walk holds

  (A) the canvas opens clean, so everything after it is caused.
  (B) ★★★★★ each of the FOUR unreadable conditions is refused **in its own
      words**, and the screen is exactly where it was. Before R1978 two of the
      four had never been driven on this screen at all.
  (C) ★★★★★ an unsound document OPENS and the sentence names its faults — the
      condition the crate now hands over, and the one this screen answers
      differently from its sibling.
  (D) ★ a whole document opens and says nothing about faults.
  (E) ★★★★★ five conditions, five distinct sentences — a refusal vocabulary
      whose members read alike is the one-word answer again with more steps.
  (F) ★ and the broken state is one a person can leave.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1978_a_saved_document_is_told_apart_by_its_condition.py
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


def refusal(app: RpcSubprocess, path: str, arg: str):
    """Answer the refusal's sentence, or None when the call went through."""
    try:
        app.invoke(path, arg)
        return None
    except Exception as why:  # noqa: BLE001 — the refusal IS the measurement
        return str(why)


def break_a_link(whole: str) -> str:
    """A saved document whose link lands on a socket that is not there.

    Built by REMOVING a card the opening graph's wires point at, so the break is
    the consequence of an ordinary-looking edit to a saved file rather than a
    hand-built fixture — the same construction R1977 used, kept so the two walks
    are talking about the same document.
    """
    envelope = json.loads(whole)
    tree = envelope["document"]["trees"][0]
    landed = {link["to"]["node"] for link in tree["links"]}
    victim = next(
        node
        for node in tree["nodes"]
        if node.get("id") in landed and isinstance(node.get("body"), dict)
    )
    tree["nodes"] = [n for n in tree["nodes"] if n.get("id") != victim["id"]]
    return json.dumps(envelope)


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

        banner("A — the canvas opens clean, so what follows is caused")
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: ★★★★★ clean to begin with — {review['fitness']!r}, "
            f"{len(review['findings'])} finding(s)",
            review["fitness"] == "clean" and not review["findings"],
        )
        whole = app.query(f"{surface}/archive")
        ok(
            f"A: ★ and the screen hands out its own document, which is what a "
            f"person's saved file IS — {len(whole)} bytes",
            len(whole) > 0 and json.loads(whole)["revision"] is not None,
        )
        cards_at_rest = app.query(f"{surface}/nodes")

        banner("B — ★★★★★ four unreadable conditions, four answers")
        # ★ The census runs over every arm the crate can refuse with, not over a
        # sample. R1977's walk drove two of these and said so; the other two had
        # never been driven on this screen, and a screen that answered all four
        # with one sentence would have passed that walk.
        unreadable = {
            # Nothing saved and an empty box: the answer to a different
            # question — "has anything been saved yet" — and a screen that said
            # "corrupt" the first time it was opened would be lying.
            "nothing has been saved": "",
            "not the envelope": "definitely not a saved graph",
            "another revision": whole.replace('"revision": 1', '"revision": 77'),
            # The envelope parses and the graph inside it does not: the taxonomy
            # moved under the file.
            "a taxonomy this build lacks": json.dumps(
                {
                    **json.loads(whole),
                    "document": {
                        **json.loads(whole)["document"],
                        "trees": [
                            {
                                **json.loads(whole)["document"]["trees"][0],
                                "nodes": [
                                    {**node, "body": "NoSuchBodyAtAll"} if i == 0 else node
                                    for i, node in enumerate(
                                        json.loads(whole)["document"]["trees"][0]["nodes"]
                                    )
                                ],
                            },
                            *json.loads(whole)["document"]["trees"][1:],
                        ],
                    },
                }
            ),
        }
        said_by = {}
        for what, text in unreadable.items():
            why = refusal(app, f"{surface}/open_graph", text)
            app.tick_ms(16)
            ok(
                f"B: ★★★★★ {what} is REFUSED — {why!r}",
                why is not None,
            )
            ok(
                f"B: ★ and the screen is where it was, so a refusal is not a "
                f"half-open — {what}",
                app.query(f"{surface}/nodes") == cards_at_rest,
            )
            said_by[what] = str(why)
        ok(
            "B: ★ and the one about a saved revision names the number it found "
            f"— {said_by['another revision']!r}",
            "77" in said_by["another revision"],
        )

        banner("C — ★★★★★ the unsound condition OPENS and is named")
        said = app.invoke(f"{surface}/open_graph", break_a_link(whole))
        app.tick_ms(16)
        said_by["unsound"] = str(said)
        ok(
            f"C: ★★★★★ a document that parsed and does not hold together is "
            f"OPENED, not refused — {said!r}",
            "opened" in str(said),
        )
        ok(
            f"C: ★★★★★ and the sentence names the faults rather than reporting "
            f"a plain success — {said!r}",
            "fault(s) the gate will name" in str(said),
        )
        review = js(app.query(f"{surface}/review"))
        ok(
            f"C: ★ and the launch gate holds it shut, so opening it is not "
            f"running it — {review['fitness']!r}, may_run={review['may_run']}",
            review["fitness"] == "stopped" and review["may_run"] is False,
        )

        banner("D — ★ a whole document opens and says nothing about faults")
        said = app.invoke(f"{surface}/open_graph", whole)
        app.tick_ms(16)
        said_by["sound"] = str(said)
        ok(
            f"D: ★★★★★ the sound condition is the quiet one — {said!r}",
            "opened" in str(said) and "fault(s)" not in str(said),
        )

        banner("E — ★★★★★ five conditions, five sentences")
        # ★ The point of publishing the three-way is that a person can act on
        # the answer, and they cannot act on an answer that reads the same as
        # the other four. This is the census in the other direction.
        distinct = set(said_by.values())
        ok(
            f"E: ★★★★★ no two conditions read alike — {len(distinct)} distinct "
            f"of {len(said_by)}: {sorted(distinct)}",
            len(distinct) == len(said_by),
        )
        ok(
            "E: ★ and 'nothing has been saved' is not spelled as corruption, "
            f"which is a different question — {said_by['nothing has been saved']!r}",
            "saved" in said_by["nothing has been saved"]
            and "not a saved graph" not in said_by["nothing has been saved"],
        )

        banner("F — ★ and the broken state is one a person can leave")
        app.invoke(f"{surface}/open_graph", break_a_link(whole))
        app.tick_ms(16)
        app.invoke(f"{surface}/open_graph", whole)
        app.tick_ms(16)
        after = js(app.query(f"{surface}/review"))
        ok(
            f"F: ★★★★★ the screen comes back — fitness {after['fitness']!r}",
            after["fitness"] == "clean",
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1978 a saved document is told apart by its condition", body)
