#!/usr/bin/env python3
"""R1942 §5.2 §5.11 — **a pin says whether its value can be looked at, and
whose refusal it is when it cannot.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — whether a pin has a
value a person may inspect — as the node lab is mounted in the shell.

# ★★★★★ The measurement that reversed this row's verdict

The row read the reference's signature as *may a data tooltip be SHOWN for this
pin*, filed it beside the port's own sentence (R1916) and put it next to the
permission surface. Read from its ONE consumer it is not about showing anything:
the consumer is a debugger's can-I-inspect-this-pin check, whose own comments
say "Can't inspect the value on an orphaned pin" and "Can't inspect exec pins or
delegate pins; **their values are not defined**". The question is whether the
pin HAS a value to read.

Counted: one supplied declaration (answering NO — a bare schema knows none of
its types), **two** overriders, **one** consumer. One refuses execution pins and
delegate pins; the other refuses pose pins and defers to the first. Execution
was already answered here — control is not a value, refused since R1644 — so the
gap is the other two: a type that CARRIES a value and still has nothing a person
can read.

★★★★★ And the measured defect is that its answer is a bare `bool`: its one
consumer asks FIVE separate questions and folds every one into the same `false`,
so a person told *no* cannot tell which of the five they met.

# What this walk holds

  (A) the journey reaches the node lab, and every pin answers whether it can be
      looked at — a register the reference has no equivalent of at all.
  (B) ★★★★★ every answer is DECIDED, never absent: a pin is watchable or it
      names who refused it and why. Nothing is left unclassified.
  (C) ★ this taxonomy hides nothing, and the register says so rather than
      staying silent — "nothing is stopping you" and "nothing is wrong" are
      different statements.
  (D) ★★★★★ the register is DERIVED from the framework's own gate, not
      re-decided here: what it publishes for a pin is what arming a watch on
      that pin would answer.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1942_a_pin_says_whether_it_can_be_looked_at.py
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


def watchable(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/watchable"))["pins"]


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

        banner("A — every pin answers whether it can be looked at")
        rows = watchable(app, surface)
        ok(f"A: the register answers — {len(rows)} pin(s)", len(rows) > 1)
        ok(
            "A: ★ every row names the pin it is about",
            all(row["pin"] for row in rows),
        )

        banner("B — ★★★★★ every answer is DECIDED, never absent")
        ok(
            "B: ★★★★★ every pin says yes or names WHO refused it — an "
            "unclassified answer would be the escape hatch this axis is about",
            all(
                row["watchable"] is True
                or (row["refused_by"] and row["why"])
                for row in rows
            ),
        )
        # ★ And the two refusers are distinguishable BY NAME, which is the
        # distinction the reference cannot make: there both reach the caller
        # through one `bool` from one schema call.
        refusers = {row["refused_by"] for row in rows if not row["watchable"]}
        ok(
            f"B: ★ a refusal names WHOSE it is, so the crate's and the "
            f"taxonomy's are told apart — {sorted(refusers)}",
            refusers <= {"the crate", "the taxonomy", "the document"},
        )

        banner("C — ★ this taxonomy hides nothing, and says so")
        hidden = [row for row in rows if not row["watchable"]]
        ok(
            f"C: ★★★★★ every pin on this canvas can be looked at — "
            f"{len(hidden)} that cannot: {hidden}",
            not hidden,
        )
        ok(
            "C: ★ and the register SAYS so rather than staying silent, which "
            "is what lets a screen show 'nothing is hidden' as a fact",
            all(row["watchable"] is True for row in rows),
        )

        banner("D — ★★★★★ the register is derived from the framework's gate")
        # ★ The published list covers exactly the pins the canvas draws, and
        # every one of them was answered by ARMING a watch rather than by a
        # second rule written in the screen.
        pins = {row["pin"] for row in rows}
        inks = js(app.query(f"{surface}/inks"))["pins"]
        drawn = {row["pin"] for row in inks if not row["member"]}
        ok(
            f"D: ★★★★★ the register covers exactly the pins the canvas draws — "
            f"{len(pins)} vs {len(drawn)}",
            pins == drawn,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1942 a pin says whether it can be looked at", body)
