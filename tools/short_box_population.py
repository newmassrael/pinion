#!/usr/bin/env python3
"""R1800 — which screens are asked whether their boxes hold their text.

## The rule this exists to keep

`pinion_core::containment::short_by` answers a question no other check in this
tree asks: not *did a mark leave the box that owns it* (that is
`scene/containment`, and it answers **escapes 0** on the screen whose clipped
descender a reader reported), but *was a run's OWN box authored tall enough for
the face it is set in*. The two are independent — a pane can be roomy while the
row inside it is four pixels short of the line it holds — and only the second
one is what a person sees as a cut `p`.

Screens ask it by calling `assert_boxes_hold_their_text`. This makes sure every
screen that could ask, does.

## Why the population is computed and not listed

R1798 is the round that paid for this lesson at full price. `r1712`'s demo held
a hand-written `SCREENS` list of three, written when the demo was; two more
screens grew a shrink policy in later rounds and were never added, and both had
been publishing the one verdict the framework calls inexcusable. **They were not
failing the check. They had never been in it.**

That is this project's recurring shape — R1738 counted the sections an
application had been judged on, R1784 found a ratchet walking four keys of six,
R1795 found a blast radius chosen by hand, R1798 found the constant above — and
the answer is the same every time: compute the population.

So the set here is derived: every workspace example whose painted-scene tests
already run the ink gate (`assert_contained_ink`) is a screen that builds a
scene and judges it, which is exactly the precondition for asking this too. A
screen in that set without `assert_boxes_hold_their_text` is reported by name.

## What it deliberately does not do

Decide any screen's budget, or run anything. The budgets are ratchet pins that
live beside each screen's own sweep, because only the sweep knows which cases it
covers. This reports them so the campaign has a meter, and fails only on a
screen that is not asked at all.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
EXAMPLES = REPO / "examples"

#: A screen that judges its painted scene calls this. It is the framework's own
#: ink gate, lifted into `pinion-core` at R1672 precisely so that every screen
#: asks the same question the same way.
INK_GATE = "assert_contained_ink"

#: And this is the question R1800 added beside it.
SHORT_GATE = "assert_boxes_hold_their_text"

#: The ratchet pin each screen carries. Reported, never judged here.
BUDGET = re.compile(r"const\s+SHORT_BOX_BUDGET\s*:\s*usize\s*=\s*([A-Za-z0-9_:]+)\s*;")


def _painted_sources(root: Path) -> list[Path]:
    """Every example's painted-scene test source, in a stable order."""
    return sorted(root.glob("*/src/painted.rs"))


def survey(root: Path) -> list[tuple[str, bool, bool, str | None]]:
    """`(screen, runs the ink gate, runs the short-box gate, its pin)`."""
    rows = []
    for path in _painted_sources(root):
        text = path.read_text(encoding="utf-8")
        # The call, not the import: a screen can import a helper and never use
        # it, which is exactly how R1672 found a screen holding a copy of the
        # ink metric that nothing ever called.
        inks = f"{INK_GATE}(" in text
        shorts = f"{SHORT_GATE}(" in text
        pin = None
        if found := BUDGET.search(text):
            pin = found.group(1)
        rows.append((path.parent.parent.name, inks, shorts, pin))
    return rows


def report(rows) -> list[str]:
    """The screens that judge their scene but are not asked this question."""
    return [name for name, inks, shorts, _ in rows if inks and not shorts]


def _selftest() -> int:
    """Both directions, on trees built here rather than on this repo.

    A tool that only ever sees a passing tree has not been shown to fail, and
    this project has now found four hand-written populations by looking. The
    negative case matters as much: a screen with NEITHER gate is not a screen
    this tool has an opinion about, and reporting it would make the check fire
    on every example that does not paint.
    """
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)

        def screen(name: str, body: str) -> None:
            src = root / name / "src"
            src.mkdir(parents=True)
            (src / "painted.rs").write_text(body, encoding="utf-8")

        screen("a-complete", f"{INK_GATE}(x);\n{SHORT_GATE}(y);\nconst SHORT_BOX_BUDGET: usize = 12;\n")
        screen("b-unasked", f"{INK_GATE}(x);\n")
        screen("c-not-a-judge", "fn main() {}\n")
        # Importing without calling is the R1672 failure and must not count.
        screen("d-imports-only", f"use screen_ink::{{{INK_GATE}, {SHORT_GATE}}};\n{INK_GATE}(x);\n")

        rows = survey(root)
        by_name = {name: row for row, name in zip(rows, [r[0] for r in rows])}

        def check(desc: str, got, want) -> None:
            nonlocal failures
            if got != want:
                failures += 1
                print(f"selftest FAIL: {desc}\n  want: {want!r}\n  got:  {got!r}")

        check("every painted source is surveyed", len(rows), 4)
        check("a screen with both gates is not reported", "a-complete" in report(rows), False)
        check("a screen missing the short-box gate IS reported", "b-unasked" in report(rows), True)
        check("a source that judges nothing is not reported", "c-not-a-judge" in report(rows), False)
        check("an import without a call does not count as asking", "d-imports-only" in report(rows), True)
        check("the pin is read back", by_name["a-complete"][3], "12")
        check("and its absence is reported as absent", by_name["b-unasked"][3], None)

    if failures:
        print(f"selftest: {failures} failure(s)")
        return 1
    print("selftest: OK")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="fail when a screen is unasked")
    ap.add_argument("--selftest", action="store_true", help="run this tool's own tests")
    args = ap.parse_args()

    if args.selftest:
        return _selftest()

    rows = survey(EXAMPLES)
    judging = [r for r in rows if r[1]]
    for name, _, shorts, pin in judging:
        state = f"pin {pin}" if shorts else "NOT ASKED"
        print(f"  {name:<26} {state}")
    print(
        f"short-box population: {len(judging)} screen(s) judge their painted scene, "
        f"{sum(1 for r in judging if r[2])} are asked whether their boxes hold their text"
    )

    unasked = report(rows)
    if args.check and unasked:
        print(
            "short-box population: these screens judge their painted scene but are "
            f"never asked this question — {', '.join(unasked)}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
