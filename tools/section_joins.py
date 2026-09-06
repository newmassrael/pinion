#!/usr/bin/env python3
"""R2044 — what a section owes the moment it JOINS the application.

## The round that paid for this

R1946-R1948 opened the rail's last two seats and built two sections for them.
R1953 was the first round to run those two against the gates every other
section already passed, and found five properties missing from each: no
keyboard stop, a `described` wire shape unlike everyone else's, no row in three
budget censuses, no shrink classification, and a painted rectangle bigger than
the pressable one.

All five were repaid there. What was not repaid is the reason nobody saw them:
the five questions live in five files, are asked by five different rounds, and
every one of them is answered only by a demo in CI — which was red for an
unrelated reason, so four pushes landed on top of the silence.

## What this asks, and why these three

The three of the five that a FILE can answer, so they can be asked at the push
gate rather than after it:

  * a row in each of the three budget censuses, which are total by construction
  * a recorded shrink expectation, because "not in the map" once read as
    "concedes nothing" for two screens that concede
  * a `descriptions()` of its own, which is the wire shape the other sections
    publish

The other two — a focus stop, and a press landing where the paint says — need
the binary running, and stay CI's.

## What it deliberately does not do

Decide any of those answers. It asks whether the section was ASKED.

## The population is derived

A section is mounted by the shell, so the shell's own manifest names every one:
a crate it does not depend on cannot be a section of it. Nothing here is a list
somebody has to remember to extend — which is the defect the whole file is
about.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHELL_MANIFEST = ROOT / "examples" / "hello-analyzer-shell" / "Cargo.toml"
SHRINK_DEMO = ROOT / "tools" / "demos" / "r1712_a_window_says_what_it_gives_up.py"

#: The censuses that are TOTAL — every example has a row or the file is wrong.
#: Named here because they are the three a section joins, not because the list
#: is the answer: each is read, and a missing row names the file it is missing
#: from.
BUDGETS = (
    "text-smear-budget.tsv",
    "containment-budget.tsv",
    "scroll-reach-budget.tsv",
)


def mounted_sections(manifest: str) -> list[str]:
    """The section crates the shell depends on, in the order it names them.

    Pure in `manifest`. A path dependency on a sibling example IS the mount:
    the shell cannot show a section whose crate it does not carry, and it
    carries nothing else.
    """
    out: list[str] = []
    for line in manifest.splitlines():
        found = re.match(r'^(hello-[a-z0-9-]+)\s*=\s*\{\s*path\s*=\s*"\.\./', line)
        if found and found.group(1) not in out:
            out.append(found.group(1))
    return out


def budget_names(text: str) -> set[str]:
    """Every example a budget census carries a row for. Pure in `text`."""
    names = set()
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        names.add(line.split("\t", 1)[0].strip())
    return names


def classified(demo_source: str) -> set[str]:
    """The screens the shrink demo records an expectation for.

    Parsed rather than grepped: a name inside a comment explaining the map is
    not a member of it, and this file's own history is why — an unclassified
    screen read as `False` for four pushes.
    """
    tree = ast.parse(demo_source)
    for node in ast.walk(tree):
        if not isinstance(node, ast.AnnAssign) or not isinstance(node.target, ast.Name):
            continue
        if node.target.id != "CONCEDES" or not isinstance(node.value, ast.Dict):
            continue
        return {
            key.value
            for key in node.value.keys
            if isinstance(key, ast.Constant) and isinstance(key.value, str)
        }
    return set()


def publishes_descriptions(source: str) -> bool:
    """Whether a section builds the `described` wire shape the others do.

    The shape itself is checked by a demo against a running binary; what is
    asked here is whether the section BUILDS one at all, which is the half that
    was missing when two sections published a bare list of tags instead.

    ⚠ The needle is the type's constructor, not a function name, and that is a
    correction this file's first draft earned: it asked for `fn descriptions()`
    and the graph lab — which has published the shape since long before any of
    this — names its own `pin_descriptions(state)`. A predicate written from
    the two screens the round was looking at called the oldest one a defect.
    """
    return "Descriptions::new()" in source


def source_of(crate: str) -> str:
    """Every Rust line of a section crate, concatenated. The oracle."""
    here = ROOT / "examples" / crate / "src"
    return "\n".join(
        path.read_text(encoding="utf-8", errors="replace")
        for path in sorted(here.rglob("*.rs"))
    )


def owed(
    sections: list[str],
    budgets: dict[str, set[str]],
    ranked: set[str],
    described: dict[str, bool],
) -> list[str]:
    """What each section joined without. Pure in its arguments."""
    faults: list[str] = []
    for name in sections:
        for census, members in budgets.items():
            if name not in members:
                faults.append(
                    f"{name} is mounted by the shell and has no row in {census} —"
                    " that census is total, so its absence is not a zero, it is a"
                    " question nobody asked"
                )
        if name not in ranked:
            faults.append(
                f"{name} is mounted and records no shrink expectation — an"
                " unclassified screen once read as *concedes nothing* for two"
                " screens that concede, through four pushes"
            )
        if not described.get(name, False):
            faults.append(
                f"{name} is mounted and publishes no `descriptions()` — two"
                " sections joined publishing a bare list of tags where every"
                " other one publishes a region and its sentences"
            )
    return faults


def report() -> int:
    sections = mounted_sections(SHELL_MANIFEST.read_text(encoding="utf-8"))
    if not sections:
        print("section joins: the shell mounts nothing — the manifest changed shape", file=sys.stderr)
        return 1
    budgets = {
        name: budget_names((ROOT / "docs" / name).read_text(encoding="utf-8"))
        for name in BUDGETS
    }
    ranked = classified(SHRINK_DEMO.read_text(encoding="utf-8"))
    described = {name: publishes_descriptions(source_of(name)) for name in sections}
    faults = owed(sections, budgets, ranked, described)
    for fault in faults:
        print(f"section joins: {fault}", file=sys.stderr)
    if faults:
        print(f"section joins: {len(faults)} thing(s) a section joined without", file=sys.stderr)
        return 1
    print(
        f"section joins: {len(sections)} mounted section(s), each with a row in"
        f" {len(BUDGETS)} census(es), a shrink expectation and a `descriptions()`"
    )
    return 0


def selftest() -> int:
    failures = 0

    def check(name: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"FAIL: {name}")

    manifest = 'hello-node-lab = { path = "../hello-node-lab" }\nserde = "1"\n'
    check("a path dependency on a sibling example is a mount", mounted_sections(manifest) == ["hello-node-lab"])
    check("and a registry dependency is not", "serde" not in mounted_sections(manifest))
    check(
        "the real shell mounts more than one section",
        len(mounted_sections(SHELL_MANIFEST.read_text(encoding="utf-8"))) > 1,
    )
    check(
        "a budget's comment lines are not members",
        budget_names("# a note\nhello-x\t0\nhello-y\tunmeasured\treason\n")
        == {"hello-x", "hello-y"},
    )
    check(
        "the classification is PARSED, so a name in a comment is not a member",
        classified(
            "#: hello-ghost is mentioned here\n"
            "CONCEDES: dict[str, bool] = {'hello-real': True}\n"
        )
        == {"hello-real"},
    )
    check(
        "a section that builds the type publishes descriptions",
        publishes_descriptions("let mut described = Descriptions::new();"),
    )
    check(
        "and the check is on the TYPE, not on a function name — the graph lab's"
        " is `pin_descriptions(state)` and it has published the shape all along",
        publishes_descriptions("fn pin_descriptions(s: &S) -> Descriptions {\n    Descriptions::new()"),
    )
    check("and a section that builds none does not", not publishes_descriptions("fn describe() -> Thing {"))
    # ★★★★★ Both directions of the rule, because the tree's own answer is
    # one-sided: every section passes today, so a rule exercised only here would
    # never have run its refusing arm.
    check(
        "a section in every census with an expectation and a function owes nothing",
        owed(["hello-x"], {"b.tsv": {"hello-x"}}, {"hello-x"}, {"hello-x": True}) == [],
    )
    missing_row = owed(["hello-x"], {"b.tsv": set()}, {"hello-x"}, {"hello-x": True})
    check("a missing census row is refused, naming the census", any("b.tsv" in f for f in missing_row))
    check(
        "an unclassified section is refused",
        any("shrink expectation" in f for f in owed(["hello-x"], {}, set(), {"hello-x": True})),
    )
    check(
        "a section with no `descriptions()` is refused",
        any("descriptions()" in f for f in owed(["hello-x"], {}, {"hello-x"}, {})),
    )
    # ★★★★★ The ORACLE, exercised against this tree rather than left to the
    # report path alone — `tools/oracle_census.py` refused this file's first
    # version for exactly that, which is the instrument doing its job. A pure
    # rule and the thing that feeds it are two gates, and the one nobody
    # remembers to test is the reader.
    lab = source_of("hello-node-lab")
    check("the source oracle answers a real section", len(lab) > 10_000)
    check(
        "and what it hands back satisfies the rule — the graph lab publishes"
        " descriptions under its own function name",
        publishes_descriptions(lab),
    )
    check(
        "a crate that is not there answers nothing rather than raising",
        source_of("hello-no-such-section-r2044") == "",
    )
    print(f"section joins selftest: {'PASS' if not failures else 'FAIL'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selftest", action="store_true", help="run this file's own tests")
    args = parser.parse_args()
    return selftest() if args.selftest else report()


if __name__ == "__main__":
    raise SystemExit(main())
