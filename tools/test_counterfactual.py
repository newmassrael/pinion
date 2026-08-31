#!/usr/bin/env python3
"""Tests for `tools/counterfactual.py` (R1600.1).

The driver exists because R1599 wrote two defects into an uncommitted copy of it
and both reported success while being wrong. Each of the six properties the
debt note demanded has an assertion here, and `pre-push` runs this file — so the
thing that verifies every round is itself verified.

    python3 tools/test_counterfactual.py
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import counterfactual as cf  # noqa: E402

FAILURES: list[str] = []


def check(condition: bool, what: str) -> None:
    if condition:
        print(f"  ok   {what}")
    else:
        print(f"  FAIL {what}")
        FAILURES.append(what)


class Completed:
    """Stand-in for `subprocess.CompletedProcess`."""

    def __init__(self, returncode: int, stdout: str = "", stderr: str = "") -> None:
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


def test_compile_error_is_not_a_catch() -> None:
    """Property 3, and the exact defect R1599 shipped.

    cargo prints `error: test failed, to rerun pass ...` for an ORDINARY
    assertion failure, so a classifier testing for `"error:"` calls a caught
    counterfactual a compile error. R1599's did, for 8 of 10 cases.
    """
    print("compile-vs-assertion classification")
    ordinary = Completed(
        101,
        stdout="---- tests::a_thing stdout ----\nassertion failed\n",
        stderr="error: test failed, to rerun pass `-p x --lib`\n",
    )
    verdict, _ = cf.classify(ordinary)
    check(verdict == cf.CAUGHT, "an assertion failure is CAUGHT, not BROKEN")
    check(
        "error:" in ordinary.stderr,
        "and the naive `'error:' in out` test would have said otherwise",
    )

    broken = Completed(
        101,
        stderr="error[E0308]: mismatched types\nerror: could not compile `x`\n",
    )
    verdict, detail = cf.classify(broken)
    check(verdict == cf.BROKEN, "a real compile error is BROKEN")
    check("E0308" in detail, "and the diagnostic line is reported")

    green = Completed(0, stdout="test result: ok. 12 passed\n")
    verdict, _ = cf.classify(green)
    check(verdict == cf.PASSED, "a green gate is PASSED — the finding case")


def test_a_scoped_gate_is_refused() -> None:
    """Property 2: R1599 scoped a case to one test, so PASSED meant only that
    THAT test did not catch it."""
    print("gate scope")
    for gate, why in [
        (["cargo", "test", "-p", "x", "--lib", "some_test_name"], "a name filter"),
        (["cargo", "test", "--exact", "a::b"], "--exact"),
        ([], "an empty gate"),
    ]:
        try:
            cf.check_gate(gate)
        except SystemExit:
            check(True, f"refused: {why}")
        else:
            check(False, f"should have refused: {why}")

    for gate in (
        ["cargo", "test", "-p", "pinion-node-graph", "--lib"],
        ["cargo", "test", "--workspace"],
        # ★ R1672 — an example package's whole suite. Every example here is its
        # own package, so this IS the un-scoped form for a screen, and it was
        # refused because `--manifest-path` was not known to consume its value.
        ["cargo", "test", "--manifest-path", "examples/hello-node-lab/Cargo.toml"],
        ["cargo", "test", "--manifest-path", "x/Cargo.toml", "-j2"],
        ["bash", "-c", "true"],
    ):
        try:
            cf.check_gate(gate)
        except SystemExit:
            check(False, f"wrongly refused a whole-suite gate: {gate}")
        else:
            check(True, f"accepted a whole-suite gate: {' '.join(gate)}")


def test_an_anchor_that_does_not_match_is_reported() -> None:
    """Property 1: R1594 lost a counterfactual to `cargo fmt` re-indenting the
    target, so the edit applied to nothing and the run reported PASS."""
    print("anchor application")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "src.rs").write_text("fn main() {\n    let a = 1;\n}\n")
        case = cf.Case(
            name="CF-x",
            file="src.rs",
            find="let a = 1;  // reformatted away",
            replace="",
            why="",
        )
        outcome = cf.run_case(root, ["bash", "-c", "true"], case)
        check(outcome.verdict == cf.NOT_APPLIED, "a missing anchor is NOT-APPLIED")
        check(
            outcome.verdict != cf.PASSED,
            "and is NOT reported as a finding — the failure mode that made "
            "an unapplied counterfactual indistinguishable from a caught one",
        )

        applied = cf.Case(
            name="CF-y", file="src.rs", find="let a = 1;", replace="", why=""
        )
        outcome = cf.run_case(root, ["bash", "-c", "true"], applied)
        check(outcome.applied == 1, "an applied anchor reports its site count")
        check(outcome.verdict == cf.PASSED, "and a green gate is then a finding")


def test_the_file_is_restored_and_the_hash_says_so() -> None:
    """Property 4: a driver that dies mid-case leaves the tree mutated."""
    print("restoration")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = "fn main() { let a = 1; }\n"
        (root / "src.rs").write_text(source)
        case = cf.Case(
            name="CF-z", file="src.rs", find="let a = 1;", replace="", why=""
        )
        # A gate that FAILS is the interesting path: the restore has to happen
        # on the way out of a failure too.
        cf.run_case(root, ["bash", "-c", "exit 1"], case)
        check((root / "src.rs").read_text() == source, "restored after a CAUGHT run")

        # And a gate that mutates the file behind the driver's back must be
        # caught by the hash rather than silently accepted.
        try:
            cf.run_case(
                root,
                ["bash", "-c", "true"],
                cf.Case(
                    name="CF-w",
                    file="src.rs",
                    find="let a = 1;",
                    replace="let a = 2;",
                    why="",
                ),
            )
        except SystemExit:
            check(False, "a clean case should not trip the hash guard")
        else:
            check(True, "a clean case does not trip the hash guard")
        check(
            (root / "src.rs").read_text() == source,
            "and the replacement was undone exactly",
        )


def test_a_red_baseline_is_refused() -> None:
    """Property 6 (R1619): a gate that is already failing must stop the run.

    This is the failure mode that reads as success — every case reports CAUGHT
    because the gate was red before anything was broken, and the summary says
    `N/N caught`. Measured for real: a killed run left a file mutated and the
    next run's eight CAUGHT verdicts were all void.
    """
    print("a red baseline is refused")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "guard.txt").write_text("WRONG\n")
        plan = {
            # Red from the start: the invariant the gate reads is not there.
            "gate": ["bash", "-c", "grep -q THE-INVARIANT guard.txt"],
            "cases": [
                {
                    "name": "CF-1 anything at all",
                    "file": "guard.txt",
                    "find": "WRONG",
                    "replace": "ALSO-WRONG",
                    "why": "the verdict would be CAUGHT for the wrong reason",
                }
            ],
        }
        (root / "plan.json").write_text(json.dumps(plan))
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "counterfactual.py"),
                str(root / "plan.json"),
                "--root",
                str(root),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        blob = completed.stdout + completed.stderr
        check("ALREADY RED" in blob, "the run refuses and says why")
        # No per-case verdict line and no tally — the refusal's own prose
        # mentions the word CAUGHT, so the check is on the driver's output
        # SHAPE rather than on a substring the message happens to contain.
        check("[cf] ok " not in blob, "and reports no verdict at all")
        check("caught" not in blob.replace("CAUGHT", ""), "nor a tally")
        check(completed.returncode != 0, "with a non-zero exit")
        check(
            (root / "guard.txt").read_text() == "WRONG\n",
            "and nothing was mutated",
        )


#: The gate the end-to-end tests below drive, as a shell one-liner.
#:
#: ★★★★★ R1939.2 — `says` decides whether the gate NAMES its failure, and the
#: two tests below differ in nothing else. Before R1939 an unreadable red still
#: counted as a catch, so a silent gate was indistinguishable here from a
#: speaking one and this file drove only the silent shape; that shape now has
#: its own verdict, and a pair of tests is what tells the two apart.
#:
#: ⚠ The speaking form prints `cargo test`'s own header rather than an invented
#: sentence: an arbitrary `bash` gate belongs to none of the four harnesses the
#: driver knows, and a test that made up a fifth vocabulary would assert
#: something no real gate says.
def guard_gate(*, says: bool) -> list[str]:
    spoken = (
        'echo "---- tests::the_invariant stdout ----"; '
        'echo "the invariant is gone"; '
    )
    return [
        "bash",
        "-c",
        f"grep -q THE-INVARIANT guard.txt || {{ {spoken if says else ''}exit 1; }}",
    ]


def test_the_driver_runs_end_to_end() -> None:
    """The whole thing, over a real plan file and a real (trivial) gate."""
    print("end to end")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "guard.txt").write_text("THE-INVARIANT\n")
        # The gate passes only while the invariant is present, which is exactly
        # the shape of a real counterfactual: break it, and the gate goes red.
        plan = {
            "gate": guard_gate(says=True),
            "cases": [
                {
                    "name": "CF-1 the invariant is load-bearing",
                    "file": "guard.txt",
                    "find": "THE-INVARIANT",
                    "replace": "nothing",
                    "why": "the gate reads it",
                },
                {
                    "name": "CF-2 a break nothing notices",
                    "file": "guard.txt",
                    "find": "\n",
                    "replace": "\n\n",
                    "why": "blank lines are not read",
                },
            ],
        }
        (root / "plan.json").write_text(json.dumps(plan))
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "counterfactual.py"),
                str(root / "plan.json"),
                "--root",
                str(root),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        out = completed.stdout
        check("CAUGHT" in out, "the load-bearing break is CAUGHT")
        check("PASSED" in out, "the inert break is PASSED")
        check("FINDING" in out, "and a PASSED case is announced as a FINDING")
        check("1/2 caught" in out, "the tally is reported")
        check(completed.returncode == 1, "and a non-caught case fails the run")
        check(
            (root / "guard.txt").read_text() == "THE-INVARIANT\n",
            "with the tree left exactly as it was",
        )


def test_a_silent_red_is_not_a_catch() -> None:
    """★★★★★ R1939.2 — a gate that goes red and NAMES NOTHING is UNREADABLE.

    The counterpart of the test above, differing only in whether the gate
    speaks. It is the end-to-end half of standing rule (6): an unclassified
    result is RED, not a pass — and before R1939 this driver had exactly that
    escape hatch, reporting `exit 1` as the detail and counting the case toward
    `caught`.

    ⚠ Why a second test rather than a wider assertion in the first: the two
    differ in ONE variable, so a regression can only be in the classifier and
    not in the plan, the anchor or the restoration. And it is the shape this
    file itself used to drive — `grep -q` says nothing — which is why the
    change was invisible until `pre-push` ran it.
    """
    print("a red gate that names nothing")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "guard.txt").write_text("THE-INVARIANT\n")
        plan = {
            "gate": guard_gate(says=False),
            "cases": [
                {
                    "name": "CF-1 the invariant is load-bearing",
                    "file": "guard.txt",
                    "find": "THE-INVARIANT",
                    "replace": "nothing",
                    "why": "the gate reads it and says nothing about it",
                },
            ],
        }
        (root / "plan.json").write_text(json.dumps(plan))
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "counterfactual.py"),
                str(root / "plan.json"),
                "--root",
                str(root),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        out = completed.stdout
        check(cf.UNREADABLE in out, "the silent red is UNREADABLE")
        check(
            "CAUGHT" not in out,
            "★★★★★ and it is NOT reported as a catch, which is what it used to be",
        )
        check("0/1 caught" in out, "so the tally counts it as uncaught")
        check(
            completed.returncode == 1,
            "and the run fails, rather than passing on an unreadable verdict",
        )
        check(
            "teach FAILURE_MARKERS this harness's vocabulary" in out,
            "★ and the repair is NAMED, so an unreadable gate is actionable",
        )
        check(
            (root / "guard.txt").read_text() == "THE-INVARIANT\n",
            "with the tree left exactly as it was",
        )


def main() -> int:
    for test in (
        test_compile_error_is_not_a_catch,
        test_a_red_baseline_is_refused,
        test_a_scoped_gate_is_refused,
        test_an_anchor_that_does_not_match_is_reported,
        test_the_file_is_restored_and_the_hash_says_so,
        test_the_driver_runs_end_to_end,
        test_a_silent_red_is_not_a_catch,
    ):
        test()
    if FAILURES:
        print(f"\ncounterfactual driver: {len(FAILURES)} failure(s)")
        return 1
    print("\ncounterfactual driver: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
