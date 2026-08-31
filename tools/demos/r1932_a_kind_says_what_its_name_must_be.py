#!/usr/bin/env python3
"""R1932 §5.12 §5.2 — **a kind says where its name has to be unique, or that it
need not be.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — *make me a name
validator for this node* — through the node lab as it is mounted in the shell.

# ★★★★★ What the reference does, measured at its header, its consumers and all
# fourteen overriders

Its graph node publishes *make me a name validator*, supplied `NULL`. Its schema
publishes a call of the same shape that is NOT the same mechanism — four
arguments, zero overriders, and consumers that name a blueprint's variables and
actions rather than a graph's nodes. Only the node-side one is about a node.

The fourteen overriders of that one do exactly two things:

  * ★★★★★ **four SUPPRESS** — a comment and both reroute classes hand back a
    validator that accepts everything, carrying the same copy-pasted remark that
    comments may be duplicated. Taking the rule AWAY is its commonest use, which
    is the shape R1928 measured on the pin-naming hook;
  * **the rest choose a SCOPE**, building their validator over the whole
    blueprint rather than over the graph the node sits in.

⇒ the axis is a scope with an off position.

# ⚠ And the census sentence was FALSE, not half false

It read *no name-validation surface: a label is free text*. A label has not been
free text since R1682: an empty name is refused, and a taken one is refused with
a message that NAMES the node holding it — where the reference's `AlreadyInUse`
is a bare enum constant that cannot. What was absent is that the rule was the
crate's alone: no application could widen the scope or turn it off, and a frame
— this crate's comment — was held to the same uniqueness as a node the graph is
addressed by.

# What this walk holds

  (A) every node on the canvas publishes what its name must be, and BOTH answers
      are present — the frames say `free`, the cards say `tree`. An axis with
      one value reached is an axis nothing checks.
  (B) ★★★★★ two FRAMES may share one caption, and after they do, both hold it.
  (C) ★★★★★ two CARDS may not, and the refusal NAMES the card already holding
      the name — the half the reference's enum result cannot carry.
  (D) an empty name is refused for both, because that rule is not about scope.
  (E) and the register is DERIVED: what it says about a node is what the rename
      then does.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1932_a_kind_says_what_its_name_must_be.py
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


def naming(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/naming"))["nodes"]


def rename(app: RpcSubprocess, surface: str, which: str, to: str):
    """Answer the refusal's sentence, or None when it was accepted."""
    try:
        app.invoke(f"{surface}/rename", f"{which},{to}")
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

        banner("A — every node says what its name must be, and BOTH answers appear")
        rows = naming(app, surface)
        ok(f"A: one row per node on the canvas — {len(rows)}", len(rows) >= 4)
        answers = {row["unique"] for row in rows}
        ok(
            f"A: ★ every answer is one the crate can give — {sorted(answers)}",
            answers <= {"tree", "document", "free"},
        )
        free = [row["card"] for row in rows if row["unique"] == "free"]
        bound = [row["card"] for row in rows if row["unique"] != "free"]
        ok(
            f"A: ★★★★★ BOTH are present on this canvas — free {free}, bound "
            f"{bound[:3]}…. An axis with one value reached is an axis nothing "
            "checks",
            free != [] and bound != [],
        )
        ok(f"A: ★ and there are two frames to collide — {free}", len(free) >= 2)

        banner("B — ★★★★★ two FRAMES may share one caption")
        first, second = free[0], free[1]
        refused = rename(app, surface, second, first)
        ok(
            f"B: ★★★★★ renaming {second!r} to {first!r} is ACCEPTED — {refused!r}",
            refused is None,
        )
        after = naming(app, surface)
        holders = [row["card"] for row in after if row["card"] == first]
        ok(
            f"B: ★ and BOTH hold it now — {len(holders)} node(s) called {first!r}",
            len(holders) == 2,
        )
        ok(
            "B: ★ their answer is still `free`, so this is the rule and not an "
            "accident",
            all(row["unique"] == "free" for row in after if row["card"] == first),
        )

        banner("C — ★★★★★ two CARDS may not, and the refusal NAMES the holder")
        target, other = bound[0], bound[1]
        why = rename(app, surface, other, target)
        ok(f"C: ★★★★★ renaming {other!r} to {target!r} is REFUSED — {why!r}", why is not None)
        ok(
            f"C: ★★★★★ and the refusal names the card already holding it — "
            f"{target!r} in {why!r}",
            target in (why or ""),
        )
        for spelling in ("NodeId(", "LabelTaken", "EditError"):
            ok(
                f"C: ★ it is a sentence, not Rust syntax — no {spelling!r}",
                spelling not in (why or ""),
            )
        still = naming(app, surface)
        ok(
            f"C: ★ and {other!r} is still called that — a refusal changed nothing",
            any(row["card"] == other for row in still),
        )

        banner("D — an empty name is refused whatever the scope")
        for who in (free[0], bound[0]):
            said = rename(app, surface, who, "   ")
            ok(
                f"D: ★ {who!r} refuses a blank name — {said!r}",
                said is not None,
            )

        banner("E — ★★★★★ the STATED cost of `free`, and the register predicts")
        # ★★★★★ The trade `free` makes, asserted rather than left implicit: a
        # name two nodes answer to does not identify one, so the screen can no
        # longer be told which of them to act on. The crate says so by
        # answering None to a by-name lookup, and here that surfaces as the
        # rename refusing to resolve the name at all. This is the honest cost of
        # the arm and not a defect — the reference takes the same trade silently.
        said = rename(app, surface, first, "anything")
        ok(
            f"E: ★★★★★ {first!r} is now held by two nodes, so naming it reaches "
            f"neither — {said!r}",
            said is not None and "no node is called" in said,
        )
        final = naming(app, surface)
        for row in final:
            if row["unique"] == "free":
                continue
            twin = next(
                (r["card"] for r in final if r["unique"] != "free" and r["card"] != row["card"]),
                None,
            )
            if twin is None:
                continue
            said = rename(app, surface, row["card"], twin)
            ok(
                f"E: ★★★★★ {row['card']!r} says {row['unique']!r} and is refused "
                f"{twin!r} — {said!r}",
                said is not None,
            )
            break

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1932_a_kind_says_what_its_name_must_be", body))
