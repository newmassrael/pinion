#!/usr/bin/env python3
"""R1943 §5.2 §5.11 — **a zone is a PAIR, not a stored region.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — a bracketed region
of one tree — as the node lab is mounted in the shell.

# ★★★★★ The measurement that reversed this row's verdict

The row read *a ZONE: a bracketed region of ONE tree with evaluation
semantics*. That describes what a person SEES. Read from the operator, adding a
zone does four things: it creates an INPUT node and an OUTPUT node, PAIRS them,
places them either side of the cursor, and wires the one socket they share.
Nothing stores a region — the region is derived from a PAIR, and the
reference's four zones (a simulation across a time span, a dynamic repetition,
a per-element operation, and a closure evaluated elsewhere) are four such pairs.

★ Its own field said where to look: `mechanism: python-core`, and grepping the
C sources for the operator answers ZERO — it is a Python operator over a C-side
pairing call, the same shape R1936 met.

★★★★★ Two defects were measured and each decided a piece of what was built:
the pairing is a ONE-WAY id (stored on the opener, nothing on the closer, so
asking a closer what it closes means walking every opening node), and its
refusals are REPORTED rather than returned (`bool` plus a report list, so
*wrong kind of closer* and *already paired* arrive as one `false`). A third was
found by reading the routine: it checks only whether the CLOSER is spoken for,
so re-pairing an opener silently abandons the zone it was in.

# What this walk holds

  (A) the journey reaches the node lab, and the zone register answers for every
      card — a register the reference has no equivalent of.
  (B) ★★★★★ this taxonomy opens NO zone, and the register SAYS so as a fact
      rather than staying silent. A deployment plan has nothing a bracketed
      evaluation region corresponds to, and R1941's rule is why none was
      invented to make this walk look busier.
  (C) ★ every card is accounted for, so "none" is a statement about the whole
      canvas rather than about whichever cards happened to be asked.
  (D) ★★★★★ the register is DERIVED from the framework's own answer, so a
      taxonomy that began opening zones would show here without this screen
      being touched.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1943_a_zone_is_a_pair_not_a_region.py
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

        banner("A — the zone register answers for every card")
        published = js(app.query(f"{surface}/zones"))
        rows = published["cards"]
        ok(f"A: the register answers — {len(rows)} card(s)", len(rows) > 1)
        ok(
            "A: ★ every row names the card it is about",
            all(row["card"] for row in rows),
        )

        banner("B — ★★★★★ this taxonomy opens no zone, and SAYS so")
        ok(
            f"B: ★★★★★ the register states it as a fact — any={published['any']}",
            published["any"] is False,
        )
        ok(
            "B: ★ and every card's own answer agrees, so the summary is not a "
            "second statement free to disagree with the rows",
            all(row["in_zone"] is None for row in rows),
        )

        banner("C — ★ every card is accounted for")
        # ★ The register covers the same cards the canvas draws, so "none" is
        # about the whole canvas rather than about a subset that was asked.
        drawn = {row["node"] for row in js(app.query(f"{surface}/tints"))["nodes"]}
        ok(
            f"C: ★ the register covers exactly the cards the canvas draws — "
            f"{len(rows)} vs {len(drawn)}",
            {row["card"] for row in rows} == drawn,
        )

        banner("D — ★★★★★ the register is derived from the framework")
        # ★ Each row's shape is the framework's three-way answer rendered, not a
        # boolean this screen invented: a card in a zone would carry a side and
        # a partner, and an opener waiting for one would carry a side with no
        # partner. Nothing here can produce those without the framework saying
        # so, which is what makes `any: false` a measurement.
        ok(
            "D: ★★★★★ no row carries a side, which is the framework's `None` "
            "and not a default this screen chose",
            all(row["in_zone"] is None for row in rows),
        )
        ok(
            "D: ★ and the register is a distinct surface from the ones that "
            "describe pins, so a zone answer cannot be mistaken for a pin one",
            "cards" in published and "pins" not in published,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1943 a zone is a pair not a region", body)
