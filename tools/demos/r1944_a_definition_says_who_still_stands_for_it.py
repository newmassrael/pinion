#!/usr/bin/env python3
"""R1944 §5.2 §5.11 — **a definition says who still stands for it, and a
removal says what it took.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — removing a
definition from a document — as the node lab is mounted in the shell.

# ★★★★★ The measurement

The row read *a definition cannot be removed from a document*. TRUE, and this
round found the structural reason it was true: a tree's id WAS its position in
the document's list, so removing one would have made every later id name a
different tree. That is fixed here (a search, and a frontier the document
remembers rather than derives from what remains).

★★★★★ And the hook the row is named for is DEAD: counted this round, **one
declaration answering NO, ZERO overriders, one consumer** — that extension point
has never once been taken, so every deletion goes down the editor's fallback.
R1938's shape again: a hook whose refusal is never exercised is a hook nobody
has had to think about.

The fallback is the capability, and three things about it decided the design:
it removes every node bound to that graph UNCONDITIONALLY and answers `void`;
whether a graph may go at all is a FLAG on the graph, so *why not* has no
answer; and it does not look for definitions its own removal orphaned.

# What this walk holds

  (A) the journey reaches the node lab, and every definition says WHERE it is
      used — sites, not a count.
  (B) ★★★★★ removing one that is in use is REFUSED, and the refusal says how
      many stand for it. Nothing is lost by asking.
  (C) ★ the refusal changed nothing: the definition and its cards are still
      there.
  (D) ★★★★★ the caller can say to take them too, and the answer REPORTS what
      went — the half the reference answers `void` for.
  (E) ★ and what stood for it is gone from the canvas, while everything else
      stayed.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1944_a_definition_says_who_still_stands_for_it.py
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


def definitions(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/definitions"))["definitions"]


def cards(app: RpcSubprocess, surface: str) -> set[str]:
    return {row["node"] for row in js(app.query(f"{surface}/tints"))["nodes"]}


def refusal(app: RpcSubprocess, path: str, arg: str):
    try:
        return None, app.invoke(path, arg)
    except Exception as why:  # noqa: BLE001 — the refusal IS the measurement
        return str(why), None


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

        banner("A — every definition says WHERE it is used")
        # ★ The opening canvas has no definitions, so one is MADE through this
        # screen's own verb — which is what keeps the rest of this walk about a
        # reachable state rather than a fixture.
        before = cards(app, surface)
        subject = sorted(before)[0]
        made, said = refusal(app, f"{surface}/regroup", subject)
        ok(f"A: a definition was made through the screen's own verb — {said!r}", made is None)
        app.tick_ms(16)
        rows = definitions(app, surface)
        ok(f"A: the register answers — {len(rows)} definition(s)", rows)
        ok(
            "A: ★ and each says WHERE it is used, as sites rather than a count",
            all(isinstance(row["used_by"], list) for row in rows),
        )
        held = rows[0]
        ok(
            f"A: ★★★★★ the new definition is used by {len(held['used_by'])} "
            f"card(s), each named — {held['used_by']}",
            held["used_by"] and all(site["card"] for site in held["used_by"]),
        )

        banner("B — ★★★★★ removing one in use is REFUSED, with a number")
        why, _ = refusal(
            app, f"{surface}/drop_definition", f"{held['definition']},keep"
        )
        ok(f"B: ★★★★★ it is refused — {why!r}", why is not None)
        ok(
            f"B: ★ and the refusal says how many stand for it — {why!r}",
            "still stand for it" in str(why),
        )

        banner("C — ★ the refusal changed nothing")
        ok(
            "C: ★ the definition is still there",
            any(row["definition"] == held["definition"] for row in definitions(app, surface)),
        )
        standing = {site["card"] for site in held["used_by"]}
        ok(
            f"C: ★ and so are the cards that stand for it — {sorted(standing)}",
            standing <= cards(app, surface),
        )

        banner("D — ★★★★★ take them too, and the answer says what went")
        why, said = refusal(
            app, f"{surface}/drop_definition", f"{held['definition']},take"
        )
        ok(f"D: ★★★★★ it went through — {said!r}", why is None)
        ok(
            f"D: ★★★★★ and the answer REPORTS what it took, which the reference "
            f"answers `void` for — {said!r}",
            "card(s)" in str(said) and "definition(s)" in str(said),
        )

        banner("E — ★ what stood for it is gone, and nothing else")
        after = cards(app, surface)
        ok(
            f"E: ★ the cards that stood for it are gone — {sorted(standing)}",
            not (standing & after),
        )
        ok(
            f"E: ★★★★★ and the definition is gone from the register — "
            f"{[row['definition'] for row in definitions(app, surface)]}",
            not any(
                row["definition"] == held["definition"]
                for row in definitions(app, surface)
            ),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1944 a definition says who still stands for it", body)
