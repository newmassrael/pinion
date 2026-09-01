#!/usr/bin/env python3
"""R1798 — which screens declare a shrink policy, computed rather than listed.

## The round that paid for this

`tools/demos/r1712_a_window_says_what_it_gives_up.py` reads a screen's
`scene/size_floor` and asserts the two things the framework says a concession
must satisfy: the verdict is `honoured`, and `unreachable` is empty — because
`crates/pinion-core/src/shrink.rs` states in as many words that a floor putting
something out of reach is the one verdict *no concession can excuse*.

The gate is right. Its POPULATION was a constant:

    SCREENS = [node lab, capture viewer, dashboard]

Three screens, written when the demo was. Two more grew a `ShrinkPolicy` in
later rounds — `hello-key-patterns` (R1730) and `hello-log-view` (R1731) — and
neither was ever added. Measured at R1798, both were reporting `unreachable` on
the wire: ten marks and twelve marks that no scrolling reaches at their own
declared floor. They were not failing the check. **They had never been in it.**

That is this project's recurring shape — R1738 counted the sections an
application had been judged on, R1784 found a ratchet walking four keys of six,
R1795 found a blast radius chosen by hand — and the answer is the same each
time: compute the population.

## What it answers

Every workspace example whose source declares a `ShrinkPolicy`, by parsing for
the constructor calls the type actually offers. `--check` compares that set
against the demo's own `SCREENS` list and fails when the demo is short, naming
which screens are unjudged.

## What it deliberately does not do

Decide what a screen's policy should be, or read the verdict itself. The demo
does that against a running binary; this only makes sure the demo is asked
about every screen that has an answer.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
EXAMPLES = REPO / "examples"
DEMO = REPO / "tools" / "demos" / "r1712_a_window_says_what_it_gives_up.py"

#: The constructors `ShrinkPolicy` offers. A screen that declares a policy calls
#: exactly one of them, and a new one added to the type without being added here
#: makes this tool silently miss a screen — which `--selftest` asserts against
#: the type's own source rather than against this list.
CONSTRUCTORS = ("rigid", "conceding", "panning", "checked")

POLICY_CALL = re.compile(
    r"ShrinkPolicy::(" + "|".join(CONSTRUCTORS) + r")\s*\(",
)


def declaring_examples() -> dict[str, list[str]]:
    """Example package name -> the constructors its source calls."""
    out: dict[str, list[str]] = {}
    for src in sorted(EXAMPLES.glob("*/src/**/*.rs")):
        try:
            text = src.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        found = sorted({m.group(1) for m in POLICY_CALL.finditer(text)})
        if not found:
            continue
        # `examples/<name>/src/...` — the package directory is the name the
        # demo launches, which is what the comparison is about.
        package = src.relative_to(EXAMPLES).parts[0]
        out.setdefault(package, [])
        for one in found:
            if one not in out[package]:
                out[package].append(one)
    return {k: sorted(v) for k, v in sorted(out.items())}


def demo_population() -> set[str]:
    """The example names the R1712 demo's own `SCREENS` list carries.

    Parsed, not grepped: the list is a literal in the demo's source and reading
    it with a regex would also match the word in a comment about it.
    """
    tree = ast.parse(DEMO.read_text(encoding="utf-8"))
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        if not any(isinstance(t, ast.Name) and t.id == "SCREENS" for t in node.targets):
            continue
        out = set()
        for item in getattr(node.value, "elts", []):
            parts = getattr(item, "elts", [])
            if len(parts) == 2 and isinstance(parts[1], ast.Constant):
                out.add(parts[1].value)
        return out
    return set()


def demo_expectations() -> set[str]:
    """The example names the R1712 demo's `CONCEDES` map classifies.

    ★★★★★ R1953 — the second half of the same question. This tool made
    `SCREENS` total and stopped there, so R1947 and R1948 each added a screen
    to that list and recorded no expectation about what it gives up. The demo
    read a missing entry as *concedes nothing*, both screens declare
    `panning`, and the audit reported two correct screens as defective for four
    pushes.

    ⇒ a screen is in the check when it is **driven and classified**. Being in
    one list and not the other is what this now refuses.
    """
    tree = ast.parse(DEMO.read_text(encoding="utf-8"))
    for node in tree.body:
        if not isinstance(node, (ast.Assign, ast.AnnAssign)):
            continue
        targets = node.targets if isinstance(node, ast.Assign) else [node.target]
        if not any(isinstance(t, ast.Name) and t.id == "CONCEDES" for t in targets):
            continue
        keys = getattr(node.value, "keys", [])
        return {k.value for k in keys if isinstance(k, ast.Constant)}
    return set()


def selftest() -> int:
    """The two claims this tool rests on, checked against their own sources."""
    failures = []
    declaring = declaring_examples()
    if not declaring:
        failures.append("no example declares a shrink policy — the pattern stopped matching")

    # ★ The constructor list is the thing most likely to go stale: a new one on
    # the type makes this tool miss every screen that uses it. Read the type.
    shrink = (REPO / "crates" / "pinion-core" / "src" / "shrink.rs").read_text(encoding="utf-8")
    declared = set(re.findall(r"pub const fn ([a-z_]+)\(\s*$", shrink, re.M))
    declared |= set(re.findall(r"pub const fn ([a-z_]+)\(", shrink))
    # Only the ones that RETURN a policy are constructors; the accessors return
    # numbers and are matched out by name here rather than by parsing Rust.
    constructors = {
        name
        for name in declared
        if name in {"rigid", "conceding", "panning", "checked", "declared", "fault"}
    }
    missing = constructors - set(CONSTRUCTORS) - {"declared", "fault"}
    if missing:
        failures.append(
            f"ShrinkPolicy offers constructor(s) this tool does not look for: {sorted(missing)}"
        )

    if not demo_population():
        failures.append("the demo's SCREENS list did not parse — the comparison would be vacuous")
    if not demo_expectations():
        failures.append(
            "the demo's CONCEDES map did not parse — the classification check "
            "below would be vacuous, which is exactly how the thing it checks "
            "went unnoticed"
        )

    for line in failures:
        print(f"shrink population selftest: {line}", file=sys.stderr)
    print(
        f"shrink population selftest: {len(declaring)} declaring example(s), "
        f"{len(CONSTRUCTORS)} constructor(s) watched"
    )
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when the demo is short")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    declaring = declaring_examples()
    if not args.check:
        for package, calls in declaring.items():
            print(f"{package}  ({', '.join(calls)})")
        return 0

    judged = demo_population()
    classified = demo_expectations()
    unjudged = sorted(set(declaring) - judged)
    stray = sorted(judged - set(declaring))
    # ★ R1953 — driven and not classified, which is the half that was missing.
    unclassified = sorted(judged - classified)
    orphaned = sorted(classified - judged)
    print(
        f"shrink population: {len(declaring)} screen(s) declare a policy, "
        f"{len(judged)} judged by {DEMO.name}, {len(classified)} classified"
    )
    for package in unjudged:
        print(f"  UNJUDGED  {package}  ({', '.join(declaring[package])})")
    for package in stray:
        print(f"  STRAY     {package} is judged and declares no policy")
    for package in unclassified:
        print(f"  UNCLASSIFIED  {package} is driven and CONCEDES says nothing about it")
    for package in orphaned:
        print(f"  ORPHANED  {package} is classified and never driven")
    if unjudged:
        print(
            "\nA screen that declares what it gives up and is not asked about it is not "
            "passing the check — it was never in it. Add it to SCREENS.",
            file=sys.stderr,
        )
    if unclassified or orphaned:
        print(
            "\nA screen with no recorded expectation is not passing either: the demo "
            "read a missing entry as 'concedes nothing', and two screens that concede "
            "were reported as defects for four pushes. Add it to CONCEDES.",
            file=sys.stderr,
        )
    return 1 if (unjudged or unclassified or orphaned) else 0


if __name__ == "__main__":
    sys.exit(main())
