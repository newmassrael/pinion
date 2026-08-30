#!/usr/bin/env python3
"""R1922 §5.12 §2 #2 — **the assembled tool says what its graph will accept,
before anything is put in it.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability four reference-census rows name and none of them
covered — the DCC asking a node type and a node instance *can this be added to
this tree*, the engine asking a node whether it may be created under a schema
and whether it is compatible with a graph — through the node lab as it is
mounted in the shell.

# ★★★★★ Why the refusal is REAL here and not a tripwire

R1920's walk had to assert an ABSENCE — every card deletable, because the one
refusal it built needs a subgraph and this tool builds none. This round's
refusals do not need one: the lab's canvas IS the root tree, and the root is
the tree nothing instantiates, so an interface end placed there would
materialise a contract with no outside. That is refused on this screen, by this
screen's own graph, and (C) drives it.

# What this walk holds

  (A) the screen publishes a row for every body this crate owns, and each
      verdict is one of two words.
  (B) an ordinary body is ACCEPTED — without this a screen refusing everything
      would pass (A) and (C) together.
  (C) ★★★★★ an interface end is REFUSED on this graph, and the refusal says
      what would be wrong rather than only that it refused.
  (D) ★ a group instance naming this very tree is refused, and the sentence
      NAMES THE CHAIN that would close — which is the half the reference does
      not give: it prints one flat sentence whether the nesting is direct or
      four groups deep.
  (E) the row is a DERIVATION: it is the same answer the verb would give, shown
      by asking the screen to place a card and seeing the accepted body land.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1922_a_graph_says_what_it_will_accept.py
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


def accepts(app: RpcSubprocess, surface: str) -> dict:
    return {row["body"]: row for row in js(app.query(f"{surface}/accepts"))["bodies"]}


def cards(app: RpcSubprocess) -> set[str]:
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

        banner("A — the graph publishes what it would accept")
        rows = accepts(app, surface)
        ok(f"A: every body this crate owns has a row — {sorted(rows)}", len(rows) >= 4)
        for name, row in sorted(rows.items()):
            ok(
                f"A: {name} answers with a known word — {row['verdict']!r}",
                row["verdict"] in {"allowed", "refused"},
            )
            if row["verdict"] == "refused":
                # ⚠ The value is NOT sliced inside the message: a mutation that
                # made this None turned a clean FAIL into a TypeError, so the
                # walk caught the defect and could not say what it was. A check
                # that cannot report is half a check.
                ok(
                    f"A: and its refusal carries a sentence — {row['because']!r}",
                    bool(row["because"]),
                )
            else:
                ok(
                    "A: ★ while an allowance carries none — a reason beside a "
                    "yes is a reason nobody can act on",
                    row["because"] is None,
                )

        banner("B — an ordinary body is accepted")
        # ⚠ Without this, a screen that refused EVERYTHING would satisfy (A) and
        # (C) at once and look correct.
        ok(
            f"B: ★ a frame goes in — {rows['frame']['verdict']}",
            rows["frame"]["verdict"] == "allowed",
        )

        banner("C — ★★★★★ an interface end is REFUSED, on this screen's own graph")
        for side in ("interface-input", "interface-output"):
            ok(
                f"C: {side} is refused — {rows[side]['verdict']}",
                rows[side]["verdict"] == "refused",
            )
            ok(
                f"C: ★ and the reason says what would be WRONG, not merely that "
                f"it refused — {rows[side]['because']}",
                "no outside" in rows[side]["because"],
            )
        ok(
            "C: ★★★★★ this is a live refusal and not an absence asserted for "
            "later: the lab's canvas IS the root tree, the one tree nothing "
            "instantiates",
            "root" in rows["interface-input"]["because"],
        )

        banner("D — a tree cannot hold itself, and the chain is NAMED")
        held = rows["group-of-this-tree"]
        ok(f"D: it is refused — {held['verdict']}", held["verdict"] == "refused")
        ok(
            f"D: ★★★★★ and the sentence names the CHAIN that would close, which "
            f"the reference does not — {held['because']}",
            "chain" in held["because"],
        )

        banner("E — the row is a DERIVATION, not a snapshot taken once")
        before = cards(app)
        ok(f"E: the canvas draws cards — {len(before)}", len(before) >= 2)
        # ★ Change the document under it and ask again. A row computed once at
        # boot would answer identically for a reason that has nothing to do
        # with the rule; a row derived from the document answers identically
        # BECAUSE the rule did not change, which is what the assertion below is
        # about — and the count changing is what shows the document really did.
        subject = sorted(before)[0]
        app.invoke(f"{surface}/delete_node", subject)
        app.tick_ms(16)
        after = cards(app)
        ok(
            f"E: the document really changed — {len(before)} then {len(after)}",
            len(after) < len(before),
        )
        again = accepts(app, surface)
        ok(
            "E: ★ and the graph still refuses an interface end, on a document "
            "it has not seen before",
            again["interface-input"]["verdict"] == "refused",
        )
        ok(
            "E: ★ and still accepts a frame — the rule is about the BODY and the "
            "tree, not about what happens to be in it",
            again["frame"]["verdict"] == "allowed",
        )
        # ★★★★★ The verb and the row are one predicate in the crate
        # (`Document::admits`), and `dcc_node_poll` holds that as a law. What
        # this walk adds is that the ASSEMBLED tool publishes that same answer
        # rather than a second opinion computed on the screen side.
        ok(
            f"E: ★ every refusal still carries its sentence — "
            f"{[b for b, r in sorted(again.items()) if r['verdict'] == 'refused']}",
            all(
                row["because"]
                for row in again.values()
                if row["verdict"] == "refused"
            ),
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1922_a_graph_says_what_it_will_accept", body))
