#!/usr/bin/env python3
"""The counterfactual driver (R1600.1).

A **counterfactual** is this project's round-level verification: break the thing
the round claims to have built, one break at a time, and check that something
fails. A break nothing catches is not a false alarm — it is a **finding**, and
the recorded lesson is that a passed counterfactual has been a real discovery
nine rounds running.

Every round runs them. Until now nothing ran them *the same way twice*: the
driver was rewritten in a scratchpad each round, and R1599 built two defects
into one, both of which reported success while being wrong. This is that driver,
committed, with the six properties those defects cost stated as assertions in
`tools/test_counterfactual.py`.

    python3 tools/counterfactual.py plan.json

`plan.json`::

    {
      "gate": ["cargo", "test", "-p", "pinion-node-graph", "--lib"],
      "cases": [
        {
          "name": "CF-1 the cut is what makes a cycle legal",
          "file": "crates/pinion-node-graph/src/model.rs",
          "find": "&& !self.cuts_dependency(tree, from.node)",
          "replace": "",
          "why": "without it, connect refuses the loop-closing wire"
        }
      ]
    }

Exit code is 0 only when every case was CAUGHT. `PASSED` and `BROKEN` are both
failures of the *round*, for opposite reasons, and both are reported by name.

## The six properties, and what each one cost

1. **The edit must actually apply.** R1594 lost a whole counterfactual to
   `cargo fmt` having re-indented the target after the anchor was written: zero
   edits were made and the run reported PASS. `find` must occur at least once,
   and how many times is printed.
2. **The gate is the whole suite, not one test.** R1599 scoped a case to
   `--lib <name>`, so `PASSED` meant "that test does not catch it" rather than
   "nothing catches it". Widening it found a real defect immediately. A `gate`
   naming a single test is refused.
3. **A compile error is a FAILURE, not a catch.** R1581's lesson is that a
   counterfactual that does not compile tests nothing; R1599's classifier was
   `"error:" in output`, and cargo prints `error: test failed` for an ordinary
   assertion failure, so 8 of 10 cases were misreported as compile errors. The
   test here is `error[E` / `could not compile` / `error: could not`.
4. **Restoration is verified by hash.** A driver that dies mid-case leaves the
   tree mutated (R1557, R1578). Every file is hashed before and after.
5. **The driver has its own tests**, in the same place `tools/test_rpc_verify.py`
   sits, and `pre-push` runs them.
6. **The baseline must be green.** R1619 killed a run mid-case, which left one
   file mutated (the R1617 lesson, hit again); the next run started from a red
   tree, and every case reported CAUGHT because the gate was already failing.
   The summary read `8/12 caught` and all eight were void. This is the only
   failure mode here that looks like success, so the gate is now run once
   **unmutated** first and a red baseline is refused.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

#: Substrings that mean the build never produced a testable binary. Deliberately
#: NOT `"error:"` — cargo prints `error: test failed, to rerun pass ...` for an
#: ordinary assertion failure, which is a CATCH.
COMPILE_MARKERS = ("error[E", "could not compile", "error: could not")

#: A gate that names one test cannot answer "does anything catch this".
SINGLE_TEST_FLAGS = ("--exact",)

CAUGHT, PASSED, BROKEN, NOT_APPLIED = "CAUGHT", "PASSED", "BROKEN", "NOT-APPLIED"


@dataclass
class Case:
    name: str
    file: str
    find: str
    replace: str
    why: str = ""


@dataclass
class Outcome:
    case: Case
    verdict: str
    detail: str
    applied: int = 0

    @property
    def ok(self) -> bool:
        return self.verdict == CAUGHT


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def classify(completed: subprocess.CompletedProcess) -> tuple[str, str]:
    """Property 3: a compile error is its own verdict, and it is a failure."""
    blob = (completed.stdout or "") + (completed.stderr or "")
    for marker in COMPILE_MARKERS:
        if marker in blob:
            line = next(
                (row for row in blob.splitlines() if marker in row), marker
            )
            return BROKEN, line.strip()[:200]
    if completed.returncode == 0:
        return PASSED, "the gate stayed green with the mechanism broken"
    failing = [
        row.strip()
        for row in blob.splitlines()
        if row.strip().startswith("---- ") or " FAILED" in row
    ]
    return CAUGHT, "; ".join(failing[:3])[:300] or f"exit {completed.returncode}"


def check_gate(gate: list[str]) -> None:
    """Property 2: refuse a gate that can only speak for one test."""
    if not gate:
        raise SystemExit("counterfactual: the plan has no gate command")
    for flag in SINGLE_TEST_FLAGS:
        if flag in gate:
            raise SystemExit(
                f"counterfactual: gate contains {flag!r}, so PASSED would mean "
                "'that test does not catch it' rather than 'nothing does'"
            )
    # A bare positional after `test` that is not a flag is a test-name filter.
    if "test" in gate:
        rest = gate[gate.index("test") + 1 :]
        skip_value = False
        for token in rest:
            if skip_value:
                skip_value = False
                continue
            if token.startswith("-"):
                skip_value = token in ("-p", "--package", "--features", "-j")
                continue
            raise SystemExit(
                f"counterfactual: gate filters to {token!r}. Run the whole "
                "suite: a scoped gate cannot say that NOTHING catches a break."
            )


def check_baseline(root: Path, gate: list[str]) -> None:
    """Property 6 (R1619): the gate must be GREEN before anything is broken.

    A red baseline makes every case report CAUGHT — the gate was already
    failing, so it fails again with the mechanism broken, and the driver cannot
    tell the two apart. Measured R1619: a killed run left one file mutated (the
    R1617 lesson, hit again), the next run started from a red tree, and **all
    eight CAUGHT verdicts were void** while the summary said `8/12 caught`.

    That is the worst failure mode this driver has, because it reads as
    success. The other four verdicts announce themselves; this one does not.
    """
    completed = subprocess.run(
        gate, cwd=root, capture_output=True, text=True, check=False
    )
    if completed.returncode != 0:
        blob = (completed.stdout or "") + (completed.stderr or "")
        failing = [
            row.strip()
            for row in blob.splitlines()
            if row.strip().startswith("---- ") or " FAILED" in row
        ]
        raise SystemExit(
            "counterfactual: the gate is ALREADY RED before any case ran, so "
            "every case would report CAUGHT for the wrong reason. Fix the "
            "baseline first (a killed earlier run may have left the tree "
            "mutated — check `git diff`).\n  "
            + ("\n  ".join(failing[:5]) or f"exit {completed.returncode}")
        )
    print("[cf] baseline green")


def run_case(root: Path, gate: list[str], case: Case) -> Outcome:
    target = root / case.file
    if not target.is_file():
        return Outcome(case, NOT_APPLIED, f"no such file: {case.file}")
    original = target.read_text()
    before = hashlib.sha256(original.encode()).hexdigest()

    hits = original.count(case.find)
    if hits == 0:
        # Property 1. The commonest cause is a formatter having re-indented the
        # anchor since it was written.
        return Outcome(
            case,
            NOT_APPLIED,
            f"anchor not found in {case.file} — the edit would have applied to "
            "NOTHING, and a run that reports PASS on that is indistinguishable "
            "from one that was caught",
        )

    target.write_text(original.replace(case.find, case.replace))
    try:
        completed = subprocess.run(
            gate,
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
        verdict, detail = classify(completed)
    finally:
        target.write_text(original)
        after = digest(target)
        if after != before:
            # Property 4.
            raise SystemExit(
                f"counterfactual: {case.file} was NOT restored "
                f"({before[:12]} -> {after[:12]}). Fix the tree before "
                "trusting anything above."
            )
    return Outcome(case, verdict, detail, hits)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path, help="the plan JSON")
    parser.add_argument(
        "--root", type=Path, default=Path.cwd(), help="workspace root"
    )
    args = parser.parse_args(argv)

    plan = json.loads(args.plan.read_text())
    gate: list[str] = plan["gate"]
    check_gate(gate)
    cases = [Case(**one) for one in plan["cases"]]

    print(f"[cf] gate: {' '.join(gate)}")
    check_baseline(args.root.resolve(), gate)
    outcomes: list[Outcome] = []
    for case in cases:
        outcome = run_case(args.root.resolve(), gate, case)
        outcomes.append(outcome)
        mark = "ok " if outcome.ok else "★  "
        print(f"[cf] {mark}{outcome.verdict:<11} {case.name}")
        print(f"[cf]     {outcome.detail}")
        if outcome.applied:
            print(f"[cf]     applied at {outcome.applied} site(s)")

    caught = sum(1 for one in outcomes if one.ok)
    print(f"[cf] {caught}/{len(outcomes)} caught")
    for one in outcomes:
        if one.verdict == PASSED:
            print(f"[cf] ★ FINDING: nothing catches {one.case.name!r} — {one.case.why}")
        elif one.verdict == BROKEN:
            print(f"[cf] ✗ {one.case.name!r} did not compile, so it tested NOTHING")
        elif one.verdict == NOT_APPLIED:
            print(f"[cf] ✗ {one.case.name!r} was never applied")
    return 0 if caught == len(outcomes) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
