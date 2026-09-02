#!/usr/bin/env python3
"""R1976 §5.2 §5.21 — **the whole document is checked once, worst first, and
every finding reaches the panel a person reads.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — the pre-launch
validation pass — as the node lab is mounted in the shell.

# ★★★★★ What R1974 left, and what R1976 measured

R1945 built `Document::review` as the JOIN of the two halves a caller needs:
the structural verdict (`validate`) and every kind's judgement (`warnings`).
R1974's carry recorded that it existed and **nothing on any screen called it**.

Measured at R1976, through this surface and by reading the screen:

  * the lab asked `warnings(ROOT)` for one half — so a judgement inside any
    tree but the root was invisible — and `validate()` for the other,
  * and of `Violation`'s SEVENTEEN arms it matched exactly ONE, dropping the
    rest with a bare `continue`.

The dropped arms are the faults a document can only ARRIVE with rather than be
edited into (the enum's own header says so), and this screen opens saved
documents. So the state was reachable and the gate said nothing about it.

# ★★★★★ Where this is ahead of the behaviour canon, measured against it

Its validation pass answers a flat array of `{card, level, field, sentence}` in
EMISSION order, with two levels, and gates a run on `level === 'error'` being
absent. Three consequences, each of which this screen does not share:

  * its jump-to-first-issue takes a person to `[0]` — whatever was raised
    first, which is the first card its walk reached and not the worst thing
    wrong. Here the order IS severity.
  * its gate is a boolean over a filter, so it cannot say *nothing stops you,
    and something was said*. `Fitness` has that arm.
  * its walk is over one graph.

# What this walk holds

  (A) the journey reaches the node lab, and the review answers for the whole
      document with a three-valued fitness.
  (B) ★★★★★ the findings are ordered WORST FIRST, and `worst` is the head of
      that same list rather than a second scan.
  (C) ★★★★★ every finding the review found is in the gate the person reads —
      the join is not re-derived and nothing is dropped between them.
  (D) ★ the counts are derived from the list rather than kept beside it.
  (E) ★ each finding says which HALF it came from, because the two differ in
      who may silence them.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1976_the_gate_reports_what_the_review_found.py
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

# The weights, lightest first — the taxonomy's own order, so "worst" is a
# maximum rather than a convention.
ORDER = {"notes": 0, "warns": 1, "blocks": 2}


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

        banner("A — the review answers for the whole document")
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: the register answers — fitness {review['fitness']!r}, "
            f"{len(review['findings'])} finding(s)",
            "fitness" in review and "findings" in review,
        )
        # ★★★★★ The opening canvas is CLEAN, and that is a good state for the
        # screen and no population at all for what follows — a register that
        # dropped everything would satisfy an empty list just as well. So the
        # findings below are CAUSED, by the gesture a person actually makes on
        # this screen: this taxonomy's judgement rule is *listening, and nothing
        # on this canvas dials it*, so taking wires away puts a card into it.
        ok(
            f"A: ★★★★★ it opens CLEAN, so everything below is caused rather "
            f"than found — {review['fitness']!r}, {len(review['findings'])} "
            f"finding(s)",
            review["fitness"] == "clean" and not review["findings"],
        )
        links = js(app.query(f"{surface}/links"))
        inbound = [row["id"] for row in links if row["to"] == "R-01"]
        ok(
            f"A: ★ the fixture: peers dial R-01, so removing them makes it "
            f"listen to nobody — {inbound}",
            len(inbound) > 0,
        )
        for link in inbound:
            app.invoke(f"{surface}/delete_link", str(link))
        app.tick_ms(16)
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: ★★★★★ and the review now has something to say — "
            f"{review['fitness']!r}, {len(review['findings'])} finding(s)",
            len(review["findings"]) > 0,
        )
        # ★★★★★ Three-valued, not a boolean. The middle arm is the statement a
        # gate that only ever says "open" cannot make.
        ok(
            f"A: ★★★★★ the fitness is one of THREE words, so 'nothing stops "
            f"you' and 'nothing is wrong' are different answers — "
            f"{review['fitness']!r}",
            review["fitness"] in ("clean", "remarked", "stopped"),
        )
        ok(
            f"A: ★ and it says whether the document may run, derived from that "
            f"word rather than counted a second time — may_run="
            f"{review['may_run']}",
            review["may_run"] == (review["fitness"] != "stopped"),
        )

        banner("B — ★★★★★ worst first, and `worst` is the head of that list")
        findings = review["findings"]
        ok(
            f"B: the review found something to order — {len(findings)}",
            len(findings) > 0,
        )
        weights = [ORDER[row["weight"]] for row in findings]
        ok(
            f"B: ★★★★★ the list descends by weight, so 'take me to the first "
            f"problem' and 'what is worst' are ONE answer — "
            f"{[row['weight'] for row in findings]}",
            all(a >= b for a, b in zip(weights, weights[1:])),
        )
        worst = review["worst"]
        ok(
            f"B: ★★★★★ and `worst` IS the head rather than a separate scan — "
            f"{worst}",
            worst is not None
            and worst["sentence"] == findings[0]["sentence"]
            and worst["weight"] == findings[0]["weight"],
        )

        banner("C — ★★★★★ every finding reaches the gate a person reads")
        gate = js(app.query(f"{surface}/gate"))
        # The panel's lines carry the card in front of the sentence, which is
        # `problems`' own shape — so a finding is present when some line ends
        # with its sentence.
        lines = [row["sentence"] for row in gate]
        missing = [
            row["sentence"]
            for row in findings
            if not any(row["sentence"] in line for line in lines)
        ]
        ok(
            f"C: ★★★★★ nothing the review found is missing from the gate — "
            f"{len(missing)} missing of {len(findings)}: {missing}",
            not missing,
        )
        # ★★★★★ And the card travels with it. A sentence with no card is the
        # defect R1688 named: "take me to the first problem" becomes
        # unanswerable without parsing a name back out of a string.
        placed = [row for row in findings if row["site"]]
        ok(
            f"C: ★ every finding that a card answers for names it, and the "
            f"panel puts that name in front — {len(placed)} placed",
            all(
                any(line.startswith(f"{row['site']} ·") for line in lines)
                for row in placed
            ),
        )

        banner("D — ★ the counts are derived from the list")
        counted = {row["weight"]: row["count"] for row in review["counted"]}
        ok(
            f"D: ★★★★★ each count equals what is in the list, rather than being "
            f"kept beside it and free to drift — {counted}",
            all(
                counted[word] == len([r for r in findings if r["weight"] == word])
                for word in ORDER
            ),
        )
        ok(
            f"D: ★ and the fitness follows the counts — {review['fitness']!r} "
            f"against {counted}",
            (review["fitness"] == "stopped") == (counted["blocks"] > 0),
        )

        banner("E — ★ each finding says which half raised it")
        halves = {row["half"] for row in findings}
        ok(
            f"E: ★ every finding declares its half, because the two differ in "
            f"who may silence them — {sorted(halves)}",
            halves and halves <= {"structure", "judgement"},
        )
        # ★ And a structural fault is always blocking, which is the framework's
        # rule rather than this screen's: a tree with a structural fault is not
        # runnable whatever every kind says.
        structural = [row for row in findings if row["half"] == "structure"]
        ok(
            f"E: ★★★★★ every structural fault blocks — {len(structural)} of them",
            all(row["blocks"] for row in structural),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1976 the gate reports what the review found", body)
