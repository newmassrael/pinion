#!/usr/bin/env python3
"""R2007 §5.2 §5.11 — **a clean check says what it was clean about.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row `script_editor::CompileBlueprint`
names, on **screen A** — the node lab, mounted whole into `hello-analyzer-shell`.

# ★★★★★ Rule (9) overturned the plan, and what it overturned was R2003's audit

That audit said the absent parts were ① a verb checking the whole document at
once, ③ the verb's activity condition, and a current/stale state. Measured
against `review.rs`'s own header, written by R1945 from this same command:

* ① was **built at R1945** (`Document::review`).
* ③ was **declined there, with a stated reason**: the reference's predicate is a
  bool with no reason — two consumers, zero overriders — answering false for a
  library graph and outside editing mode while telling a person neither. Here
  the findings ARE the reason and the verdict is reported rather than hidden
  behind a greyed control.
* the current/stale clause is declared TRUE and staying true, because a graph
  here is interpreted and there is no build product to be out of date with.

So the round's work was a JUDGEMENT plus the one gap the re-measurement found.

# ★★★★★ That gap

A review said what it FOUND and not what it LOOKED AT, so *clean* and *nothing
was asked* were one answer. The reference has the same hole and cannot close it:
its compile body is wrapped in a guard on *is there a document at all* with no
else, so an empty results log is
empty whether it walked ten thousand nodes or returned at its first guard.

# What this walk holds

  (A) the journey reaches the node lab, and the review register answers.
  (B) ★★★★★ the check is CLEAN, and it says what it was clean ABOUT — the
      population is published beside the verdict.
  (C) ★★★★★ TWO populations, published apart rather than reconciled: the
      review's is the DOCUMENT's — every node of every tree — and this canvas
      draws two frames behind the cards rather than as cards. The first draft
      of this walk asserted they were EQUAL and failed at ten against eight.
  (D) ★ both halves are accounted for — the structural one says it ran, and the
      judgement one names the trees it was asked over.
  (E) ★★★★★ the population MOVES with the document, which is what makes it a
      measurement rather than a constant somebody wrote down.
  (F) ★ every finding names the cards it is on, and the counts come from the
      list — the two divergences the reference's own log has.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2007_a_clean_check_says_what_it_looked_at.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402

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

        banner("A — the review register answers")
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: it carries the verdict, the findings and the counts — "
            f"{sorted(review)}",
            {"fitness", "findings", "counted", "covered"} <= set(review),
        )

        banner("B — ★★★★★ clean, and it says what it was clean ABOUT")
        ok(
            f"B: the canon topology checks clean — fitness={review['fitness']}",
            review["findings"] == [],
        )
        covered = review["covered"]
        ok(
            f"B: ★★★★★ and the population is published beside the verdict, so "
            f"this `clean` is a measurement — {covered}",
            covered["cards"] > 0,
        )

        banner("C — ★★★★★ TWO populations, published apart rather than reconciled")
        # ★★★★★ MEASURED, and the first draft of this walk asserted they were
        # EQUAL and failed: the review's population is the DOCUMENT's — every
        # node of every tree, which is what the judgement half is really asked
        # about — and this canvas draws two frames BEHIND the cards rather than
        # as cards. Ten nodes, eight cards. Reconciling them would make one of
        # the two numbers a lie; publishing both is what lets a reader see that
        # the check looked at more than the canvas shows.
        drawn = {row["node"] for row in js(app.query(f"{surface}/tints"))["nodes"]}
        ok(
            f"C: ★ the canvas's own card list and its tints agree, so `drawn` "
            f"is this screen's population — {covered['drawn']} vs {len(drawn)}",
            covered["drawn"] == len(drawn),
        )
        ok(
            f"C: ★★★★★ and the review looked at MORE than the canvas draws, "
            f"which is the fact neither number alone can carry — "
            f"{covered['cards']} nodes vs {covered['drawn']} cards",
            covered["cards"] > covered["drawn"],
        )

        banner("D — ★ both halves are accounted for")
        ok(
            f"D: ★ the structural half says it ran — structure="
            f"{covered['structure']}",
            covered["structure"] is True,
        )
        ok(
            f"D: ★ and the judgement half names the trees it was asked over — "
            f"trees={covered['trees']}",
            covered["trees"] >= 1,
        )

        banner("E — ★★★★★ the population moves with the document")
        cards = [row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]]
        removed, refused = None, []
        for card in cards:
            try:
                app.invoke(f"{surface}/delete_node", card)
                removed = card
                break
            except RpcError as exc:
                refused.append((card, str(exc)))
        ok(
            f"E: a card came off the canvas — {removed!r} after {len(refused)} "
            f"refused",
            removed is not None,
        )
        app.tick_ms(16)
        after = js(app.query(f"{surface}/review"))["covered"]
        ok(
            f"E: ★★★★★ and the stated population followed it — "
            f"{covered['cards']} -> {after['cards']}",
            after["cards"] == covered["cards"] - 1,
        )
        ok(
            "E: ★ the tree count did not move, because a card is not a tree",
            after["trees"] == covered["trees"],
        )

        banner("F — ★ findings name cards, and the counts come from the list")
        now = js(app.query(f"{surface}/review"))
        ok(
            "F: ★ every finding names at least one card, so none of them lands "
            "on nothing",
            all(row["cards"] for row in now["findings"]),
        )
        summed = sum(row["count"] for row in now["counted"])
        ok(
            f"F: ★ and every finding is counted under exactly one weight — "
            f"{summed} vs {len(now['findings'])}",
            summed == len(now["findings"]),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r2007 a clean check says what it looked at", body)
