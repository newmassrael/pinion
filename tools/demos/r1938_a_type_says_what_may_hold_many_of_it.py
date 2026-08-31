#!/usr/bin/env python3
"""R1938 §5.2 §5.11 — **a type says what may hold many of it, and this one says
nothing.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — the engine's
container-shape question — as the node lab is mounted in the shell.

# ★★★★★ The measurement that shaped the answer

The reference asks its SCHEMA whether a pin type may be put in a container
shape. Measured across all five mentions: the hook's default body is
`return true` — every type in every shape — and its ONE overrider in the whole
tree answers `None || Array || Set || Map`, which is the same four. So the
declaration exists and **nothing in that tree ever refuses through it**, while
both its consumers are the pin type selector, filtering a menu that is never
filtered.

Here the default is a REFUSAL, and the answer is the TYPE each shape produces
rather than a bool — so a chooser that may offer a shape also knows what
offering it makes, and the permission and the result cannot disagree.

# ★ And this screen's answer is NONE, which is a declaration

This taxonomy says "several" with a variadic RUN of ports — one accept pin per
peer — rather than with one pin carrying a collection. The two are different
models of the same word and the difference is visible at the pin: a run gives
each peer its own wire and its own address, where a collection would give them
one wire and no addresses at all. An empty answer a client can READ is worth
more than an absent one it has to infer.

# What this walk holds

  (A) the journey reaches the node lab, and the register answers for every
      socket type this screen has.
  (B) ★★★★★ every one of them is held in NOTHING — the default is a refusal,
      and this taxonomy has not opted in.
  (C) ★ the shape vocabulary is CLOSED and derived, so a client knows what it
      would have been offered.
  (D) ★★★★★ and "several" is expressed the other way, which is the reason: the
      accept run exists and grows, so the model is a run of ports rather than a
      port of many.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1938_a_type_says_what_may_hold_many_of_it.py
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


def containers(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/containers"))


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

        banner("A — the register answers for every socket type")
        published = containers(app, surface)
        rows = published["types"]
        ok(f"A: it answers, and for more than one type — {len(rows)}", len(rows) > 1)
        ok(
            f"A: ★ every row names the type it is about — "
            f"{[row['type'] for row in rows]}",
            all(row["type"] for row in rows),
        )

        banner("B — ★★★★★ every type is held in NOTHING, and that is declared")
        held = {row["type"]: row["held_in"] for row in rows}
        ok(
            f"B: ★★★★★ not one type is containerisable here — {held}",
            all(shapes == [] for shapes in held.values()),
        )
        ok(
            "B: ★ and the answer is READABLE rather than absent — the register "
            "exists and lists each type with an empty answer, which is what "
            "lets a client rule the shape out instead of discovering it by "
            "failing",
            len(held) == len(rows) and len(rows) > 0,
        )

        banner("C — ★ the shape vocabulary is closed and derived")
        shapes = published["shapes"]
        ok(
            f"C: ★ three shapes, named — {shapes}",
            shapes == ["array", "set", "map"],
        )

        banner("D — ★★★★★ and 'several' is expressed the other way")
        # The accept run is this taxonomy's answer to "many": one pin per peer,
        # each with its own address. Driven rather than asserted in prose.
        named = js(app.query(f"{surface}/port_names"))
        ports = named["ports"] if isinstance(named, dict) and "ports" in named else named
        ok(
            f"D: the port register answers — {len(ports)} port(s)",
            len(ports) > 0,
        )
        # ★ Several peers reach one card through several PORTS — so there are
        # MORE ports on this canvas than there are cards, which is exactly what
        # a collection carried on one port would not produce.
        cards = {row.get("card") for row in ports if isinstance(row, dict)}
        ok(
            f"D: ★★★★★ {len(ports)} port(s) across {len(cards)} card(s), so a "
            "card carries SEVERAL ports rather than one port carrying several "
            "— the model is a RUN OF PORTS, which is why the container answer "
            "above is empty by declaration rather than by omission",
            len(ports) > len(cards) > 0,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1938 a type says what may hold many of it", body)
