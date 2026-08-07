#!/usr/bin/env python3
"""R1588 — tests for the shared focus population.

A population derivation that nothing checks is the defect this module exists to
remove, one level up: R1518's curated list was wrong for fifty rounds because
nothing asserted anything about the list itself. So the derivation gets the same
treatment the demos it feeds get.

Run from the workspace root:
    python3 tools/test_binding_focus.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from binding_focus import (  # noqa: E402
    INTERACTIVE_ROLES,
    build_command,
    NO_RPC_STDIN,
    Population,
    interactive_bindings,
)

FAILURES: list[str] = []
CHECKS = 0


def check(condition: bool, what: str) -> None:
    global CHECKS  # noqa: PLW0603
    CHECKS += 1
    if not condition:
        FAILURES.append(what)


def main() -> int:
    pop = interactive_bindings()

    # Non-vacuity, both halves. A scan that silently stopped matching would
    # otherwise read as "the tree has no such bindings" — which is how a
    # curated list fails, wearing a derivation's clothes.
    check(bool(pop.declared), "the attribute scan matched at least one binding")
    check(bool(pop.hand_written), "the AriaRole scan matched at least one binding")
    check(
        len(pop.hand_written) > len(pop.declared),
        f"the hand-written half ({len(pop.hand_written)}) outnumbers the "
        f"declared one ({len(pop.declared)}) — R1570.5's finding, and if it "
        f"ever inverts the AriaRole scan has quietly narrowed",
    )

    # The population is a SET of names: one process per binding.
    walk = pop.walkable
    check(len(walk) == len(set(walk)), "walkable has no repeats")
    check(walk == sorted(walk), "walkable is ordered, so a run is reproducible")
    check(
        len(walk) == len({d.name for d in pop.declared} | set(pop.hand_written)),
        "walkable is exactly the union of the two halves",
    )

    # The two halves are disjoint: a binding that declares its role is asked the
    # precise question and must not also be asked the weak one.
    check(
        not ({d.name for d in pop.declared} & set(pop.hand_written)),
        "no binding is in both halves",
    )

    # Exclusions are real and are not silently in the population.
    for name in NO_RPC_STDIN:
        check(name not in walk, f"{name} is excluded by name, not walked")
    check(
        len(pop.excluded_by_name) == len(NO_RPC_STDIN),
        "every name-exclusion is reported",
    )
    for name, role in pop.excluded_by_role:
        check(
            role not in INTERACTIVE_ROLES,
            f"{name} is excluded for a genuinely non-operable role, got {role!r}",
        )
        check(name not in walk, f"{name} is excluded by role, not walked")

    # Every declared entry is well formed, and its role is one we act on.
    for d in pop.declared:
        check(bool(d.tag), f"{d.name} declares a non-empty tag")
        check(d.role in INTERACTIVE_ROLES, f"{d.name} declares an operable role")
        check(
            (Path("examples") / d.name / "src" / "main.rs").exists(),
            f"{d.name} is a real example directory",
        )

    # Every walked binding exists on disk — a name that has been renamed or
    # deleted must fail here rather than as a mystifying boot failure inside a
    # demo.
    for name in walk:
        check(
            (Path("examples") / name / "src" / "main.rs").exists(),
            f"{name} is a real example directory",
        )

    # R1588 — the build line is DERIVED from the same population, or it is a
    # third list to drift. A counterfactual pinning it to one package passed
    # every other check here, because nothing read it.
    cmd = build_command()
    check(cmd.startswith("cargo build --release "), "the build line builds in release")
    for name in walk:
        check(f"-p {name}" in cmd, f"the build line covers {name}")
    check(
        cmd.count("-p ") == len(walk),
        f"the build line names exactly the population ({cmd.count('-p ')} vs {len(walk)})",
    )

    # The summary is the line the demos print; a population that cannot describe
    # itself is one nobody audits.
    check("binding(s)" in Population().summary(), "an empty population still describes itself")

    print(f"[test] {pop.summary()}")
    print(f"[test] {CHECKS} checks")
    for f in FAILURES:
        print(f"[test] FAIL: {f}")
    if FAILURES:
        print(f"[test] {len(FAILURES)} FAILURE(S)")
        return 1
    print("[test] PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
