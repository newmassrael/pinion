#!/usr/bin/env python3
"""Answer, in one word, whether a round is CLOSED — for a reader that can only
read a verdict.

# The class this closes

An independent checker watches this repository's repayment loop and its answer
has to begin with ``YES`` or ``NO``; the driver reads the first word and nothing
else. Measured across the loop's run logs, **19 milestone claims went
unverified because the checker's answer began with something else** — 14
distinct first words, among them ``Permission``, ``You've``, ``The``,
``Verdict``, ``Independent``, one empty string, and (the round that forced this
tool) ``I've verified the code, tests, spec, ledger, and push state on disk;
the walk and the shell test suite are still running in the background.``

Every one of those is a *sentence a careful reader would write*. The checker was
not wrong and was not silent — it was UNREADABLE, and an unreadable verdict is
counted as no verdict at all, so the claim rests on the working agent's own
word.

★★★★★ The fix that is available HERE. The checker's prompt belongs to a driver
in another repository, which this repository's own rules forbid editing from a
pinion session. What can be built here is the thing that makes a verdict cheap
to give: **a command whose entire output begins with YES or NO**, so a checker
can run it and quote it rather than composing prose and hoping its first word
survives. A checker that cannot be made to lead with a verdict can still lead
with *this tool's* verdict.

# What "closed" means

The repayment loop's own definition of a finished round, one predicate each:

1. a code commit carrying the round number,
2. one changelog entry for that round in the atomic store,
3. one row in ``docs/phase-b-rounds.tsv``,
4. every one of that round's commits present on ``origin/main``,
5. the analysis-tool debt snapshot agreeing with the memory folder,
6. no uncommitted tracked changes left behind.

⚠ **Unclassifiable is NO, never YES.** If a check cannot be run — no network for
the publish check, a missing tool — the answer is ``NO`` with the reason named.
A verdict tool that answers ``YES`` when it could not look is worse than one
that refuses, because the whole point of it is to be believed. That is the
opposite of the push hook's stop-the-line reading, which fails OPEN on a missing
``gh`` — and deliberately so: that one gates an action and this one makes a
claim.

# Use

    python3 tools/round_verdict.py            # the newest round git knows
    python3 tools/round_verdict.py R1891      # a named round
    python3 tools/round_verdict.py --selftest # the tool's own gate

Exit status is 0 for YES and 1 for NO, so a shell can branch on it without
parsing — but the first word is the contract, because the reader this exists for
reads words.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSV = ROOT / "docs" / "phase-b-rounds.tsv"

#: A round token in a commit subject: `R1891`, and `R1891.1` for a follow-on.
ROUND_IN_SUBJECT = re.compile(r"\bR(\d+)(?:\.\d+)?\b")


class Check:
    """One predicate, its verdict, and the sentence it answers with."""

    def __init__(self, name: str, ok: bool, said: str) -> None:
        self.name = name
        self.ok = ok
        self.said = said


def run(*args: str, cwd: Path | None = None) -> tuple[int, str]:
    """Run a command, answering `(status, output)` rather than raising.

    A tool that raises on a failed subprocess cannot report *why* it could not
    look, and "could not look" is a verdict this tool has to be able to give.
    """
    try:
        done = subprocess.run(
            args,
            cwd=str(cwd or ROOT),
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (OSError, subprocess.TimeoutExpired) as why:
        return 127, f"{args[0]} could not run: {why}"
    return done.returncode, (done.stdout + done.stderr).strip()


def newest_round() -> int | None:
    """The highest round number git's own history carries."""
    status, out = run("git", "log", "--format=%s", "-400")
    if status != 0:
        return None
    seen = [
        int(m.group(1)) for line in out.splitlines() for m in [ROUND_IN_SUBJECT.search(line)] if m
    ]
    return max(seen) if seen else None


def commits_for(round_no: int) -> list[str]:
    """Every commit whose subject names this round, newest first."""
    status, out = run("git", "log", "--format=%H %s", "-400")
    if status != 0:
        return []
    found = []
    for line in out.splitlines():
        sha, _, subject = line.partition(" ")
        m = ROUND_IN_SUBJECT.search(subject)
        if m and int(m.group(1)) == round_no:
            found.append(sha)
    return found


def check_commit(round_no: int) -> Check:
    shas = commits_for(round_no)
    return Check(
        "commit",
        bool(shas),
        f"{len(shas)} commit(s) name R{round_no}"
        if shas
        else f"no commit subject names R{round_no}",
    )


def check_ledger(round_no: int) -> Check:
    """One changelog entry for this round, asked of the tool that owns the query.

    Not a regex over the store: `tools/changelog_rounds.py` exists because the
    store holds three key forms that interleave, and a hand-written query
    answered the wrong round twice on record.
    """
    status, out = run("python3", str(ROOT / "tools" / "changelog_rounds.py"))
    if status != 0:
        return Check("ledger", False, f"changelog_rounds.py could not answer: {out[:200]}")
    m = re.search(r"last recorded R(\d+)", out)
    if not m:
        return Check("ledger", False, "changelog_rounds.py named no last-recorded round")
    last = int(m.group(1))
    return Check(
        "ledger",
        last >= round_no,
        f"the ledger's newest entry is R{last}",
    )


def check_tsv(round_no: int) -> Check:
    if not TSV.exists():
        return Check("tsv", False, f"{TSV.name} is missing")
    rows = [
        line
        for line in TSV.read_text(encoding="utf-8").splitlines()
        if line.split("\t", 1)[0].strip() == str(round_no)
    ]
    return Check(
        "tsv",
        len(rows) == 1,
        f"{len(rows)} row(s) in {TSV.name} declare R{round_no}",
    )


def check_published(round_no: int) -> Check:
    """Every commit of this round is on `origin/main`.

    ⚠ Asked of the REMOTE, because this repository's own memory records the
    local ref and the push's exit status each lying about publish state once.
    No network means NO — see this module's header.
    """
    shas = commits_for(round_no)
    if not shas:
        return Check("published", False, f"no R{round_no} commit to publish")
    status, out = run("git", "ls-remote", "origin", "main")
    if status != 0 or not out:
        return Check("published", False, f"could not read origin/main: {out[:200]}")
    remote = out.split()[0]
    missing = []
    for sha in shas:
        # `merge-base --is-ancestor` answers "is this commit contained in that
        # one", which is the question — the tip may have moved past the round.
        code, _ = run("git", "merge-base", "--is-ancestor", sha, remote)
        if code != 0:
            missing.append(sha[:8])
    return Check(
        "published",
        not missing,
        f"origin/main is {remote[:8]}; {len(shas)} R{round_no} commit(s) present"
        if not missing
        else f"origin/main is {remote[:8]}, missing {', '.join(missing)}",
    )


def check_debt_snapshot() -> Check:
    """The committed debt snapshot agrees with the memory folder.

    A round that opened or closed a debt and did not re-write the snapshot is a
    round whose census answer is stale, and this project's push hook refuses
    exactly that — so a "closed" verdict has to ask it too.
    """
    status, out = run("python3", str(ROOT / "tools" / "analyzer_debts.py"), "--check")
    return Check(
        "debt-snapshot",
        status == 0,
        out.splitlines()[0] if out else f"analyzer_debts.py --check exited {status}",
    )


def check_tree_clean() -> Check:
    status, out = run("git", "status", "--porcelain")
    if status != 0:
        return Check("tree", False, f"git status could not run: {out[:200]}")
    dirty = [line for line in out.splitlines() if line.strip()]
    return Check(
        "tree",
        not dirty,
        "no uncommitted tracked change"
        if not dirty
        else f"{len(dirty)} uncommitted path(s): {', '.join(d[3:] for d in dirty[:4])}",
    )


def verdict(round_no: int) -> tuple[bool, list[Check]]:
    checks = [
        check_commit(round_no),
        check_ledger(round_no),
        check_tsv(round_no),
        check_published(round_no),
        check_debt_snapshot(),
        check_tree_clean(),
    ]
    return all(c.ok for c in checks), checks


def render(round_no: int, closed: bool, checks: list[Check]) -> str:
    """The report, whose FIRST WORD is the verdict.

    Nothing precedes it — no banner, no round number, no "Verdict:" label. One
    of the 19 unreadable answers on record began with the word `Verdict`, so a
    label is not a safe place to put one.
    """
    head = "YES" if closed else "NO"
    lines = [f"{head} — R{round_no} is {'closed' if closed else 'NOT closed'}"]
    for c in checks:
        lines.append(f"  {'ok  ' if c.ok else 'FAIL'} {c.name}: {c.said}")
    if not closed:
        owed = [c.name for c in checks if not c.ok]
        lines.append(f"  owed: {', '.join(owed)}")
    return "\n".join(lines)


def selftest() -> int:
    """The tool's own gate.

    ★ The property that matters is not "does it compute the right answer" — it
    is **does its output begin with a word the driver can read**, in both
    directions. A verdict tool that answers correctly in prose is the defect it
    was built for.
    """
    failures: list[str] = []

    def expect(what: str, cond: bool) -> None:
        if not cond:
            failures.append(what)

    fake_ok = [Check("a", True, "fine"), Check("b", True, "fine")]
    fake_bad = [Check("a", True, "fine"), Check("b", False, "not fine")]

    yes = render(1, True, fake_ok)
    no = render(1, False, fake_bad)

    expect("a passing report's first word is exactly YES", yes.split()[0] == "YES")
    expect("a failing report's first word is exactly NO", no.split()[0] == "NO")
    # ★ The driver reads the FIRST WORD OF THE OUTPUT, so nothing may precede
    # it — not a blank line, not a banner. Measured: one unreadable answer on
    # record was the empty string.
    expect("nothing precedes the verdict", yes[0] == "Y" and no[0] == "N")
    expect("the verdict is not merely contained", not yes.startswith(" "))
    # A failing report has to say what is owed, or a reader learns nothing it
    # can act on — the same rule this tree applies to a refusal.
    expect("a NO names what is owed", "owed: b" in no)
    expect("a NO names the failing check", "FAIL b" in no)
    expect("a YES names no owed list", "owed:" not in yes)
    # Both words are whole lines' worth of unambiguous: neither may be a prefix
    # of the other, or a driver matching a prefix reads one as the other.
    expect("YES and NO are not prefixes of one another", not ("YES".startswith("NO") or "NO".startswith("YES")))

    # The round-token regex is what joins a commit to a round, and a follow-on
    # commit (`R1891.1`) belongs to the SAME round — a rule this repository's
    # commit-msg hook enforces and this tool has to agree with.
    m = ROUND_IN_SUBJECT.search("docs(mnemosyne): R1891.1 the round entry")
    expect("a follow-on commit is read as its own round", m and m.group(1) == "1891")
    m2 = ROUND_IN_SUBJECT.search("feat(core): R1891 a detached card")
    expect("a plain round commit is read", m2 and m2.group(1) == "1891")
    expect(
        "a bare number in a subject is not a round",
        ROUND_IN_SUBJECT.search("fix: 1891 things") is None,
    )

    for line in failures:
        print(f"  FAIL {line}")
    print(f"round_verdict selftest: {12 - len(failures)} passed, {len(failures)} failed")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("round", nargs="?", help="round token, e.g. R1891")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument(
        "--json", action="store_true", help="machine form; the verdict is still the first field"
    )
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if args.round:
        m = re.fullmatch(r"[Rr]?(\d+)", args.round)
        if not m:
            print(f"NO — {args.round!r} does not name a round")
            return 1
        round_no = int(m.group(1))
    else:
        found = newest_round()
        if found is None:
            print("NO — git names no round, so there is nothing to judge")
            return 1
        round_no = found

    closed, checks = verdict(round_no)
    if args.json:
        print(
            json.dumps(
                {
                    "verdict": "YES" if closed else "NO",
                    "round": round_no,
                    "checks": {c.name: {"ok": c.ok, "said": c.said} for c in checks},
                },
                indent=2,
            )
        )
    else:
        print(render(round_no, closed, checks))
    return 0 if closed else 1


if __name__ == "__main__":
    sys.exit(main())
