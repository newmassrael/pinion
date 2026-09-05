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

   **So write the break as a WRONG answer, never an absent one.** The class has
   been walked into five times and each time the cause was different, which is
   why this is authoring guidance rather than one rule:

   * an unused binding (`unused variable`) — R1687, R1694;
   * an unread field (`dead_code`) — R1694;
   * **deleting a helper's only call site**, which makes the helper itself dead
     code under `-D warnings` — R1697. `Self::open_float_grab` was called from
     exactly one arm; removing that call did not test the arm, it stopped the
     crate compiling;
   * **an IMPORT whose only user was the line you replaced** — R1747, and the
     newest. `if painted_regions(QUERY_TAG).is_some()` was rewritten to ask a
     different store, which is a fine lie — and it left `use
     pinion_core::painted::painted_regions` with no user, so the crate stopped
     compiling under `-D warnings`. The eye is on the expression and the `use`
     line is eighty lines above it. **Before writing a case, check whether the
     replaced text is the last user of an import as well as of a binding.** The
     repair was the usual one: invert the call instead of replacing it
     (`.is_none()`), which keeps every name used and is still exactly one lie.
   * **a PARAMETER whose only use was the line you replaced** — R1712, walked
     into TWICE in one round. Replacing
     `declared.floor() != Some(policy.floor())` with `false` and
     `…(window_id, floor.0, floor.1)` with the ceiling each left an argument
     nothing read. A parameter is easy to miss here because the eye is on the
     expression, not on the signature above it: **before writing a case, look at
     what the replaced text is the last reader OF.**
   * **the only place a TYPE was inferred from** — R2019, and this one is not a
     warning at all, it is `E0282`. `let mut left = Vec::new();` gets its
     element type from the single `left.push(*role)` further down; a case that
     replaced that push with a different statement left the vector with nothing
     to infer from and the crate stopped compiling. Note the shape: the replaced
     line was not the last USER of a name, it was the last thing that TYPED one,
     which the "check what this line is the last reader of" habit does not
     catch. The repair was again the usual one — keep the push and add the lie
     beside it, so the report stays truthful while the value it reports about is
     wrong, which is a sharper case than the deletion would have been.

   The repair is the same in all of them: keep every name used and make it do
   the wrong thing — invert the comparison, swap the two sizes, clamp with the
   value instead of using it. Grab a panel named `"nothing"`; carry to the point the grab
   opened (a zero delta); raise a name that is not a panel; pass `None` where
   the real focus went. Each compiles, each is exactly one lie, and each is a
   thing the gate can actually catch.
4. **Restoration is verified by hash.** A driver that dies mid-case leaves the
   tree mutated (R1557, R1578). Every file is hashed before and after.

   ⚠ **DO NOT EDIT A FILE THIS PLAN NAMES WHILE THE RUN IS IN FLIGHT.**
   Restoration writes back the snapshot taken at the start of the case, so an
   edit made in between is silently discarded — and `git status` says nothing,
   because the file was already modified. R1709 lost three doc edits to
   `vello_capture.rs` that way and only noticed because the harness reprinted
   the file. The restore is CORRECT; what is wrong is editing underneath it.
   Wait for the run, or work in a file the plan does not touch.
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

#: `cargo test` flags that CONSUME the next argument, so that argument is not a
#: test-name filter.
#:
#: ★ R1672 — `--manifest-path` was missing, and this project keeps every example
#: in its own package: `cargo test --manifest-path examples/<x>/Cargo.toml` is
#: that package's WHOLE suite and was refused as if the path were a test name.
#: The refusal is loud, so nothing was silently mis-scoped — but the effect was
#: that a screen's own gate could not be a counterfactual gate at all, and the
#: screens are where three of this round's breaks are visible. The list is
#: spelled out rather than inferred because a flag missing from it fails
#: closed (a refusal), which is the safe direction.
VALUE_FLAGS = (
    "-p",
    "--package",
    "--exclude",
    "--features",
    "-j",
    "--jobs",
    "--manifest-path",
    "--target",
    "--target-dir",
    "--profile",
)

#: ★★★★★ R1939 — the vocabularies a gate reports a failure in, keyed by the
#: HARNESS that speaks each one.
#:
#: A gate here is whatever the plan names, and this tree has four kinds. Until
#: R1939 only `cargo test` was known, so every walk-gated case reported its
#: catch as a bare `exit 1` — the verdict right and UNREADABLE, which is the
#: class [[debt-a-checkers-verdict-is-unreadable-more-often-than-it-is-wrong]]
#: names.
#:
#: ⚠ Keyed by harness rather than flattened into one list, because what goes
#: stale here is a HARNESS nobody taught it, and a flat list cannot be asked
#: which one is missing. R1939 shipped this knowing TWO and was caught by its
#: own new refusal not knowing a third: the first mutation run against
#: [`selftest`] reported UNREADABLE rather than CAUGHT, because a tool's own
#: `--selftest` is a harness too and this driver is one of twenty.
#:
#: ⚠ A marker earns its place by being the line that NAMES the failure, not by
#: appearing in a failing run. `[demo] FAIL:` carries the assertion's own
#: sentence; `[demo]` alone would match the banner too.
#:
#: ⚠⚠ ★★★★★ The harnesses must be DISJOINT, and a `^` prefix anchors a marker
#: to the start of the line so they can be. R1939 first wrote this table with
#: an unanchored `FAIL: ` for the hook harness and a comment saying overlap was
#: harmless. It is not: `FAIL: ` also matches `[demo] FAIL: `, so a
#: counterfactual that deleted the WHOLE walk vocabulary left the gate GREEN —
#: the hook harness answered for the walk harness, and the per-harness table
#: became decorative. `selftest` now asserts that each harness is the only
#: thing that reads its own failure line.
FAILURE_MARKERS_BY_HARNESS = {
    # The per-test stdout header, and the summary list.
    "cargo test": ("---- ", " FAILED"),
    # A demo walk through `tools/rpc_verify.run_demo`: an assertion, a dead
    # peer, anything else.
    "demo walk": ("[demo] FAIL:", "[demo] RPC ERROR:", "[demo] UNEXPECTED:"),
    # A tool's own `--selftest`, in the three spellings this tree uses.
    "python --selftest": ("selftest: FAIL", "selftest FAIL", "SELFTEST FAIL"),
    # `tools/test_hooks.sh`'s `ok`, which is what gates the hook libraries.
    # Anchored: unanchored it would swallow the walk harness's line too.
    "test_hooks.sh": ("^FAIL: ",),
}

#: Flattened, for the reader that only needs "does this line name a failure".
FAILURE_MARKERS = tuple(
    marker
    for markers in FAILURE_MARKERS_BY_HARNESS.values()
    for marker in markers
)

CAUGHT, PASSED, BROKEN, NOT_APPLIED = "CAUGHT", "PASSED", "BROKEN", "NOT-APPLIED"

#: ★★★★★ R1939 — the gate went red and NOTHING in its output says what failed.
#:
#: Its own verdict rather than a `CAUGHT` with a thin detail, because a catch
#: nobody can read is not evidence: it cannot be told from a gate that fell over
#: for an unrelated reason, and the next person cannot act on it. Standing rule
#: (6) is the general form — an unclassified result is RED, not a pass — and
#: this is where this driver had its escape hatch: the detail fell back to
#: `exit N` and the case still counted toward `caught`.
UNREADABLE = "UNREADABLE"


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


def names_a_failure(row: str, marker: str) -> bool:
    """Whether `row` names a failure under `marker`.

    A marker beginning `^` anchors to the start of the stripped line — see
    [`FAILURE_MARKERS_BY_HARNESS`] for the measurement that made anchoring
    necessary rather than tidy.
    """
    row = row.strip()
    return row.startswith(marker[1:]) if marker.startswith("^") else marker in row


def failing_lines(blob: str, markers: tuple[str, ...] = ()) -> list[str]:
    """The lines of a gate's output that NAME what failed.

    ★ R1939 — one function and not the same comprehension written twice.
    `classify` and `check_baseline` both need this, and before R1939 each
    carried its own copy of the cargo-test vocabulary, so teaching one about a
    harness would have left the other blind. A reader of the baseline refusal
    and a reader of a case's detail must be shown the same sentences.

    `markers` defaults to every harness's; it is a parameter so [`selftest`]
    can ask what ONE harness's absence would cost, which is the only way to
    show that each entry in the table is load-bearing.
    """
    markers = markers or FAILURE_MARKERS
    return [
        row.strip()
        for row in blob.splitlines()
        if any(names_a_failure(row, marker) for marker in markers)
    ]


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
    failing = failing_lines(blob)
    if not failing:
        # ★★★★★ R1939 — the gate is red and its output names nothing. NOT a
        # catch with a thin detail: an unreadable verdict cannot be told from a
        # gate that fell over for an unrelated reason, so it is reported as its
        # own failure and does not count toward `caught`.
        return UNREADABLE, (
            f"exit {completed.returncode}, and no line of the gate's output "
            f"names what failed — known harnesses: "
            f"{', '.join(FAILURE_MARKERS_BY_HARNESS)}"
        )
    return CAUGHT, "; ".join(failing[:3])[:300]


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
                skip_value = token in VALUE_FLAGS
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
        failing = failing_lines(blob)
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


def selftest() -> int:
    """★★★★★ R1939 — assert the CLASSIFIER, which had no test of its own.

    This driver is a gate on other gates, and standing rule (7) says a gate
    lives only where a mutation shows it going red. Its verdicts had no such
    demonstration: the vocabulary that reads a failure was a comprehension
    inline in two functions, and nothing anywhere exercised it.

    Each assertion below names the property, not the implementation, so a
    rewrite of `classify` keeps them meaningful.
    """
    failures: list[str] = []

    def held(what: str, condition: bool) -> None:
        if not condition:
            failures.append(what)

    def ran(returncode: int, out: str) -> tuple[str, str]:
        return classify(
            subprocess.CompletedProcess(["gate"], returncode, out, "")
        )

    # (1) Each harness's own vocabulary is READ, and the detail quotes the
    # sentence that names the failure rather than an exit code.
    verdict, detail = ran(101, "---- tests::a_thing stdout ----\npanicked")
    held("a cargo-test failure is CAUGHT", verdict == CAUGHT)
    held("and its detail names the test", "tests::a_thing" in detail)

    verdict, detail = ran(1, "[demo] r1939\n[demo] FAIL: the pin took it anyway")
    held("a demo walk's failure is CAUGHT", verdict == CAUGHT)
    held(
        "★★★★★ and its detail carries the WALK'S OWN sentence, which is the "
        "half a bare exit code cannot say",
        "the pin took it anyway" in detail,
    )

    verdict, detail = ran(1, "counterfactual selftest: FAIL (1 failure(s))")
    held("a tool's own --selftest failure is CAUGHT", verdict == CAUGHT)
    held(
        "★ and a tool that names the failing case is quoted with it",
        "the escape hatch"
        in ran(1, "selftest: FAIL — the escape hatch is back")[1],
    )

    verdict, detail = ran(1, "FAIL: a hook library said the wrong thing")
    held("a test_hooks.sh failure is CAUGHT", verdict == CAUGHT)
    held(
        "★ and its detail carries the assertion's description",
        "a hook library" in detail,
    )

    # ★★★★★ The population, not just the members: every harness this table
    # claims to know must actually contribute a marker. A key with an empty
    # tuple would read as coverage and provide none — the empty-population
    # failure this project keeps meeting.
    held(
        "★★★★★ every declared harness contributes at least one marker",
        all(FAILURE_MARKERS_BY_HARNESS.values()),
    )
    held(
        "★ and the flat view is exactly the union, so a harness added to the "
        "table reaches the classifier without a second edit",
        set(FAILURE_MARKERS)
        == {
            marker
            for markers in FAILURE_MARKERS_BY_HARNESS.values()
            for marker in markers
        },
    )

    # ★★★★★ And every harness must be NEEDED, which is the assertion the
    # earlier draft could not make. Remove one harness's markers and its own
    # failure line must stop being read; if another harness still reads it, the
    # per-harness table is decorative and deleting a whole vocabulary leaves the
    # gate green. Measured R1939: an unanchored `FAIL: ` did exactly that to the
    # walk harness, and the counterfactual that removed the walk vocabulary
    # reported PASSED.
    samples = {
        "cargo test": "---- tests::a_thing stdout ----",
        "demo walk": "[demo] FAIL: the pin took it anyway",
        "python --selftest": "counterfactual selftest: FAIL (1 failure(s))",
        "test_hooks.sh": "FAIL: a hook library said the wrong thing",
    }
    held(
        "★ the sample table names every declared harness, so no harness is "
        "exempt from the check below by being left out of it",
        set(samples) == set(FAILURE_MARKERS_BY_HARNESS),
    )
    for harness, sample in samples.items():
        held(
            f"★ {harness!r}'s own failure line IS read while it is declared",
            failing_lines(sample),
        )
        without = tuple(
            marker
            for name, markers in FAILURE_MARKERS_BY_HARNESS.items()
            if name != harness
            for marker in markers
        )
        held(
            f"★★★★★ {harness!r} is the ONLY thing that reads its own failure "
            "line, so deleting it cannot leave the gate green",
            not failing_lines(sample, without),
        )

    # (2) ★★★★★ The escape hatch: red with nothing readable is its own verdict.
    verdict, detail = ran(1, "some tool that says nothing about what broke\n")
    held(
        "★★★★★ an unreadable red is UNREADABLE and not CAUGHT",
        verdict == UNREADABLE,
    )
    held(
        "and it says which harnesses it DOES know, so the repair is named",
        all(name in detail for name in FAILURE_MARKERS_BY_HARNESS),
    )
    held(
        "★ and it does not count as a catch",
        not Outcome(Case("x", "f", "a", "b"), verdict, detail).ok,
    )

    # (3) The verdicts that were already here still hold, so this test speaks
    # for the whole classifier rather than only for what R1939 added.
    held("a green gate is PASSED", ran(0, "test result: ok")[0] == PASSED)
    held(
        "a compile error is BROKEN and not a catch",
        ran(101, "error[E0599]: no method named `takes`")[0] == BROKEN,
    )

    # (4) ★ One vocabulary, two readers: the baseline refusal and a case's
    # detail must be able to quote the same line.
    blob = "[demo] FAIL: the sentence"
    held(
        "★ the baseline reader and the case reader share one extractor",
        failing_lines(blob) and failing_lines(blob)[0] in ran(1, blob)[1],
    )

    for one in failures:
        print(f"selftest: FAIL — {one}")
    print(f"counterfactual selftest: {'FAIL' if failures else 'PASS'} "
          f"({len(failures)} failure(s))")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "plan", type=Path, nargs="?", help="the plan JSON"
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="assert this driver's own classifier and exit",
    )
    parser.add_argument(
        "--root", type=Path, default=Path.cwd(), help="workspace root"
    )
    args = parser.parse_args(argv)

    if args.selftest:
        return selftest()
    if args.plan is None:
        raise SystemExit("counterfactual: a plan is required (or --selftest)")

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
        elif one.verdict == UNREADABLE:
            print(
                f"[cf] ✗ {one.case.name!r} went red and the gate said nothing "
                "readable — teach FAILURE_MARKERS this harness's vocabulary"
            )
    return 0 if caught == len(outcomes) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
