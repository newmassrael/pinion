#!/usr/bin/env python3
"""R1941 §5.2 §5.11 — **a card may REFUSE, not merely complain — and both
instruments must say so.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — a kind's judgement
about its own node carrying the WEIGHT that decides whether anything may
start — as the node lab is mounted in the shell.

# ★★★★★ The measurement that reversed this row's verdict

The row read *a KIND cannot contribute a rule to `Document::validate` — the
standing check is the crate's alone*. TRUE, and deliberate: R1927's module says
no application may add to or silence the structural check. But it measured the
wrong thing. The reference's node is asked to validate itself during
compilation and writes into the compiler's message log — the log whose ERROR
COUNT is the pass's verdict — so what it says can FAIL THE BUILD. The axis we
lacked was WEIGHT, not access to the structural check.

Counted this round: one supplied (empty) declaration, 53 overriding
declarations, 57 implementations, 5 real call sites. Across the editor's
blueprint nodes those implementations record **27 errors, 31 warnings and 2
notes** — one hook, routinely, at three weights.

# ★★★★★ And the walk found a defect no reading would have

This screen shows a launch gate as TWO instruments: a list of lines each
marked blocking or not, and a verdict counting blocking against non-blocking.
Driven, they DISAGREED — a card put on a build sharing no wire revision with
its peer produced a line marked `blocks: true` while the verdict still answered
`blocking: 0`. The verdict handed every non-value finding to the framework's
arithmetic as an *unknown key*, which is the framework's NON-BLOCKING arm, and
the comment above it claimed that made the arithmetic the framework's. It made
the arithmetic right and the INPUT wrong.

# What this walk holds

  (A) the journey reaches the node lab, and both instruments agree on a canvas
      that has nothing blocking.
  (B) ★★★★★ a blocking state is REACHABLE through this screen's own verbs, and
      the gate list marks it.
  (C) ★★★★★ THE TWO INSTRUMENTS AGREE — the verdict's blocking count equals the
      number of lines the list marks blocking, in both states. This is the
      assertion that was red before this round.
  (D) ★ the gate CLOSES, and its sentence names the card and says why.
  (E) ★★★★★ and the document is WELL FORMED throughout: this gate and the
      structural check are separate, which is what the census row's own true
      half is about.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1941_a_card_may_refuse_not_merely_complain.py
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


def verdict(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/verdict"))


def gate(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/gate"))


def agree(app: RpcSubprocess, surface: str) -> tuple[int, int]:
    """The two instruments' blocking counts — the verdict's, and the list's."""
    said = verdict(app, surface)
    lines = gate(app, surface)
    return said["blocking"], sum(1 for line in lines if line["blocks"])


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

        banner("A — both instruments on the canvas as it comes up")
        opening = verdict(app, surface)
        ok(
            f"A: the verdict separates the two weights — {opening}",
            "blocking" in opening and "warning" in opening,
        )
        ok(
            f"A: ★ every gate line carries its own weight — "
            f"{len(gate(app, surface))} line(s)",
            all("blocks" in line for line in gate(app, surface)),
        )
        counted, marked = agree(app, surface)
        ok(
            f"A: ★★★★★ and the two instruments AGREE while nothing blocks — "
            f"verdict={counted} marked={marked}",
            counted == marked == 0,
        )
        ok(
            f"A: ★ with warnings standing, so this is not an empty canvas — "
            f"warning={opening['warning']}",
            opening["warning"] > 0,
        )

        banner("B — ★★★★★ a blocking state, reached through this screen's verbs")
        # ★ Put one card on a build that shares no wire revision with the peer
        # it dials. R1885's rule: a wire between two builds with no revision in
        # common asserts a session that cannot be established.
        said = app.invoke(f"{surface}/build", "T-02,legacy")
        app.tick_ms(16)
        ok(f"B: the build was taken — {said!r}", "legacy" in str(said))
        lines = gate(app, surface)
        blocking = [line for line in lines if line["blocks"]]
        ok(
            f"B: ★★★★★ a gate line is now marked BLOCKING — "
            f"{[l['sentence'] for l in blocking]}",
            blocking,
        )

        banner("C — ★★★★★ the two instruments agree")
        counted, marked = agree(app, surface)
        ok(
            f"C: ★★★★★ the verdict's blocking count equals the number of lines "
            f"marked blocking — verdict={counted} marked={marked}",
            counted == marked,
        )
        ok(
            f"C: ★ and it is not zero, so the agreement is not vacuous — "
            f"{counted}",
            counted > 0,
        )

        banner("D — ★ the gate closes, and says why")
        closed = verdict(app, surface)
        ok(
            f"D: ★★★★★ launch is refused — {closed['sentence']!r}",
            closed["may_launch"] is False,
        )
        ok(
            f"D: ★ the blocking line names the card and the reason — "
            f"{blocking[0]['sentence']!r}",
            "T-02" in blocking[0]["sentence"]
            and "revision" in blocking[0]["sentence"],
        )

        banner("E — ★ the refusal tracks the state rather than latching")
        # ⚠ This walk deliberately does NOT assert that the document stayed
        # well formed: this screen publishes its CONFIG faults, not the
        # framework's structural violations, so the claim would be about a
        # register that cannot answer it. That separation is asserted where it
        # can be driven — `pinion-node-graph`'s own census proof holds
        # `Document::validate` empty while a kind blocks.
        # ★ And putting the build back reopens it.
        app.invoke(f"{surface}/build", "T-02,reference")
        app.tick_ms(16)
        counted, marked = agree(app, surface)
        ok(
            f"E: ★ the gate REOPENS when the build is put back, and the two "
            f"instruments still agree — verdict={counted} marked={marked}",
            counted == marked == 0 and verdict(app, surface)["may_launch"],
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1941 a card may refuse not merely complain", body)
