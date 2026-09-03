#!/usr/bin/env python3
"""R1920 §5.12 §2 #2 — **the assembled tool says what an agent may do, before
the agent does it.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability three reference-census rows name and none of them
covered — *can this node be deleted*, *can this node be renamed*, *can this
graph take new nodes* — through the node lab as it is mounted in the shell.

# ★★★★★ The property, and why it is the whole design

Both references implement the QUESTION and the EDIT as separate code: a `Can…`
predicate beside the `Delete` that re-decides. The two are then free to
disagree, and nothing there can notice. Here they are ONE decision —
`Document::may` IS the test, and `remove_node` / `relabel` / `add_node` each
begin by asking it — so `editable` cannot promise something the verb then
refuses.

That is a claim about the WIRE too, and a claim about the wire has to be driven
over the wire. §2 #2 makes the RPC path the AI's primary one, so "an agent can
plan a destructive edit without performing one to find out" is only true if the
answer an agent reads is the answer the verb will give.

# What this walk holds

  (A) the screen publishes the row, and it covers EVERY card it draws — a
      permission surface that skips a card is worse than none, because the
      card it skipped is the one nobody checked.
  (B) every published verdict is one of the two words, and a refusal carries
      its own sentence while an allowance carries none.
  (C) ★ THE LAW, over the wire: what `editable` promised is what `delete_node`
      then does. Driven on a real card, which really goes.
  (D) the row is a DERIVATION — the deleted card leaves it, with no second
      command.
  (E) a name no card carries is refused BY NAME rather than silently allowed.
  (F) ⚠ A TRIPWIRE. Every card on this screen is deletable, because the one
      refusal that exists — a tree's own interface end — needs a subgraph and
      this tool builds none. RED here is the day the assembled tool can, and
      whoever makes that day come has to assert the refusal here instead.
      Registered as `debt-the-assembled-tool-cannot-open-a-subgraph`.

The refusal itself is exercised by `pinion-node-graph::engine_may_edit`, which
drives asking-versus-doing over every node of every tree.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1920_an_agent_asks_before_it_edits.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

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


def editable(app: RpcSubprocess, surface: str) -> dict:
    return {row["node"]: row for row in js(app.query(f"{surface}/editable"))["nodes"]}


def cards(app: RpcSubprocess) -> set[str]:
    """Every card the canvas actually draws, by name."""
    return {
        tag.removeprefix("lab.node.")
        for tag in abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
        if tag.startswith("lab.node.") and tag.count(".") == 2
    }


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

        banner("A — the row covers every card that is drawn")
        drawn = cards(app)
        ok(f"A: the canvas draws cards to ask about — {len(drawn)}", len(drawn) >= 2)
        rows = editable(app, surface)
        # ★ The population is READ OFF THE FRAME, not off the same wire field
        # being checked. A surface that publishes its own idea of what exists
        # would otherwise be asserted against itself and could skip a card
        # without this noticing.
        ok(
            f"A: ★ every drawn card has a verdict — {sorted(drawn - set(rows))} "
            "missing",
            drawn <= set(rows),
        )

        banner("B — a verdict is one of two words, and a refusal explains itself")
        for name, row in sorted(rows.items()):
            ok(
                f"B: {name} answers with a known word — {row['delete']!r}",
                row["delete"] in {"allowed", "refused"},
            )
            if row["delete"] == "refused":
                ok(
                    f"B: and its refusal carries a sentence — {row['because']!r}",
                    bool(row["because"]),
                )
            else:
                ok(
                    "B: ★ while an allowance carries NO sentence — a reason "
                    "beside a yes is a reason nobody can act on",
                    row["because"] is None,
                )

        banner("C — ★ THE LAW over the wire: what was promised is what happens")
        subject = sorted(name for name, row in rows.items() if row["delete"] == "allowed")[0]
        ok(f"C: {subject} was promised deletable", rows[subject]["delete"] == "allowed")
        app.invoke(f"{surface}/delete_node", subject)
        app.tick_ms(16)
        after = cards(app)
        ok(
            f"C: ★★★★★ and it really goes — {len(drawn)} then {len(after)}",
            subject not in after and after == drawn - {subject},
        )

        banner("D — the row is a DERIVATION, not a stored list")
        now = editable(app, surface)
        ok(
            "D: ★ the deleted card leaves the row with no second command",
            subject not in now,
        )
        ok(
            f"D: and every card that remains still has a verdict — "
            f"{sorted(after - set(now))} missing",
            after <= set(now),
        )

        banner("E — a name no card carries is refused by name")
        try:
            app.invoke(f"{surface}/delete_node", "no card is called this")
            refused = False
            said = ""
        except Exception as why:  # noqa: BLE001 - the refusal is the subject
            refused = True
            said = str(why)
        ok(f"E: ★ the verb refuses an unheld name — {said[:90]}", refused)
        ok("E: and says the name back", "no card is called this" in said)

        banner("F — ★★★★★ the one refusal that exists, DRIVEN")
        # ⚠★★★★★ R1981 — this used to assert that every card on this screen was
        # deletable and to say it would go RED on the day the assembled tool
        # could open a subgraph. That day came and it did NOT go red: the
        # assertion is about the graph THIS walk builds, which has no subgraph
        # in it. A tripwire in a fixture nobody deepens is a comment, not a gate.
        #
        # A permission walk has to drive the permission. `EditError::InterfaceEnd`
        # — a tree's own interface end may not be deleted — is this crate's only
        # per-node refusal and it lives inside a subgraph, so this walk makes one.
        verdicts = {row["delete"] for row in now.values()}
        ok(
            f"F: ★ out here every card is deletable, which is the control — "
            f"{sorted(verdicts)}",
            verdicts == {"allowed"},
        )
        pair = sorted(after)[:2]
        app.invoke(f"{surface}/select", pair[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", pair[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", "inner")
        app.tick_ms(16)
        app.invoke(f"{surface}/enter", "inner")
        app.tick_ms(16)
        inside = editable(app, surface)
        refused_rows = sorted(
            name for name, row in inside.items() if row["delete"] == "refused"
        )
        ok(
            f"F: ★★★★★ and inside a subgraph a card REFUSES — {refused_rows}, of "
            f"{sorted(inside)}. What this screen publishes is no longer a constant",
            refused_rows,
        )
        blocked = ""
        try:
            app.invoke(f"{surface}/delete_node", refused_rows[0])
        except Exception as why:  # noqa: BLE001 - the refusal is the subject
            blocked = str(why)
        ok(
            f"F: ★★★★★ the ROW and the ACT agree, and the act quotes the model — "
            f"{blocked[:110]}",
            "interface" in blocked.lower(),
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1920_an_agent_asks_before_it_edits", body))
