#!/usr/bin/env python3
"""Answer, in one word, whether a round is CLOSED — for a reader that can only
read a verdict.

# The class this closes

An independent checker watches this repository's repayment loop and its answer
has to begin with ``YES`` or ``NO``; the driver reads the first word and nothing
else. Milestone claims keep going unverified because the answer begins with
something else — each of them a *sentence a careful reader would write*. The
checker was not wrong and was not silent; it was UNREADABLE, and an unreadable
verdict is counted as no verdict at all, so the claim rests on the working
agent's own word.

★ **How many, and of what shape, is a command — do not read it off this page.**

    python3 tools/round_verdict.py --census

R1892 wrote the count into this docstring and into the hook beside it, and eight
rounds later both were wrong in the same direction: the number had grown and a
*new shape* had appeared that neither sentence could have described. This is the
repository's own standing rule about a number in prose, met inside the tool
built to make a number readable.

⚠⚠ The census splits the failures by **whether a prompt could reach them**, and
that split is the point. An answer with words in it can be fixed by requiring
the checker to lead with a verdict. An answer with *no words at all* — an empty
string, or a bare line-break tag — cannot: there is nothing to put a verdict in
front of. Asking "is there a path to zero for this predicate" of the handoff's
first requirement, the answer for that bucket is **no**, and only the parser
half (treat unreadable as a dispute rather than as consent) reaches it.

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
    python3 tools/round_verdict.py --line     # EXACTLY one line, nothing else
    python3 tools/round_verdict.py --census   # the unreadable answers on record
    python3 tools/round_verdict.py --selftest # the tool's own gate

Exit status is 0 for YES and 1 for NO, so a shell can branch on it without
parsing — but the first word is the contract, because the reader this exists for
reads words.

★ ``--line`` exists because the cheapest repair on record is *"run this command
and paste its output"*, and the default report is seven lines. Seven lines
invite a reader to choose which one to quote, and choosing is the step that has
gone wrong every time. One line removes the step.
"""

from __future__ import annotations

import argparse
import json
import os
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


def run(*args: str, cwd: Path | None = None, raw: bool = False) -> tuple[int, str]:
    """Run a command, answering `(status, output)` rather than raising.

    A tool that raises on a failed subprocess cannot report *why* it could not
    look, and "could not look" is a verdict this tool has to be able to give.

    ★ `raw` keeps the output EXACTLY as the command wrote it. The default strips
    surrounding whitespace, which is right for a sentence and wrong for a
    format whose first column is data: measured at R1893, stripping
    `git status --porcelain` ate the leading space of a ` M path` line, so the
    first offending path was reported as `rates/…` instead of `crates/…`. A
    verdict tool that misnames the thing it is refusing over is one nobody
    should believe, so the convenience is opt-out rather than universal.
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
    out = done.stdout + done.stderr
    return done.returncode, out if raw else out.strip()


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
    # `raw`, because porcelain's first two columns ARE data — see `run`.
    status, out = run("git", "status", "--porcelain", raw=True)
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


# ── The census of unreadable checker answers ────────────────────────────────
#
# ⚠ THIS READS ANOTHER PROGRAM'S LOG. The line shape and the directory belong to
# the loop driver, which lives in a repository a pinion session must not edit —
# so this reader can go blind without anything breaking. That is why "no records
# found" is a **NO** here rather than a clean bill: a census that answers
# "nothing wrong" when its source has moved is worse than one that refuses.

#: How the driver writes an answer it could not read: the answer inside escaped
#: quotes, and then the driver's own sentence about it.
#:
#: ★★★★★ R1901 — the first draft was ``(.*)$`` and it was WRONG IN THE DIRECTION
#: THAT HIDES THE FINDING. The driver writes ``the checker answered \"<br>\",
#: and nothing in that reply is a YES or a NO — fix its prompt…``, so taking the
#: rest of the line appended the driver's own prose to every answer and made
#: **every** record classify as `prose`. The census then reported `contentless
#: 0` — erasing exactly the bucket this round exists to name, and disagreeing
#: with a hand count that was right.
#:
#: ⇒ the classifier was correct and the EXTRACTOR was not, which is why
#: `extract_answers` is tested against verbatim driver lines rather than only
#: against strings this file made up.
CHECKER_ANSWERED = re.compile(r'the checker answered \\"(.*?)\\"')

#: Markup that stands in for an answer the checker did not give. Stripped before
#: asking whether anything was said, because a bare line-break tag has letters
#: in it and is nonetheless nothing a person wrote.
MARKUP = re.compile(r"<[^>]*>")


def classify(answer: str) -> str:
    """Which of three buckets an answer falls in. There is no fourth.

    ``verdict``     the first token is exactly ``YES`` or ``NO`` — readable.
    ``prose``       words, but not a verdict first. A prompt requirement reaches
                    this bucket: the checker wrote a sentence and can be told to
                    lead with the verdict instead.
    ``contentless`` nothing was said at all once markup and punctuation are
                    removed. **A prompt requirement does not reach this bucket**,
                    because there is nothing to put a verdict in front of.

    ★ Case matters, and that is measured rather than chosen: the driver reads
    ``YES``/``NO`` exactly, so a lower-case ``yes`` is prose here — it would be
    prose to the driver too, and calling it readable would be this tool
    disagreeing with the reader it exists to serve.
    """
    body = answer.strip().strip('\\"').strip('"').strip()
    said = re.sub(r"[^\w]", "", MARKUP.sub("", body))
    if not said:
        return "contentless"
    first = body.split()[0].strip('"').strip("\\")
    return "verdict" if first in {"YES", "NO"} else "prose"


def extract_answers(text: str) -> list[str]:
    """Every checker answer a run log records, as the checker gave it.

    Its own function because the bug this round found was HERE and not in
    [`classify`] — see the note on [`CHECKER_ANSWERED`].
    """
    return [m.group(1) for m in CHECKER_ANSWERED.finditer(text)]


def census_logs(from_dir: Path | None) -> tuple[Path, list[Path]]:
    """Where the driver's run logs are, and which ones exist."""
    if from_dir is None:
        runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
        from_dir = Path(runtime) / "loop"
    return from_dir, sorted(from_dir.glob("run*.log")) if from_dir.is_dir() else []


def census(from_dir: Path | None) -> tuple[bool, str]:
    """Classify every checker answer the driver recorded, and report.

    ⚠ **A numerator with no denominator, stated rather than hidden.** Measured
    at R1901: the driver logs an answer only when it could not read one, and no
    log shape records a readable answer at all. So this counts failures and
    cannot produce a rate, and saying so is the difference between a census and
    a number somebody will later divide by something it does not measure.
    """
    where, logs = census_logs(from_dir)
    if not logs:
        return False, (
            f"NO — could not look: {where} holds no run log, so this is not a "
            f"clean bill. The driver's log directory is another program's; when "
            f"it moves, this refuses rather than reporting zero."
        )
    buckets: dict[str, list[str]] = {"verdict": [], "prose": [], "contentless": []}
    for log in logs:
        for answer in extract_answers(log.read_text(errors="replace")):
            buckets[classify(answer)].append(answer)
    total = sum(len(v) for v in buckets.values())
    if total == 0:
        return False, (
            f"NO — could not look: {len(logs)} run log(s) under {where} and not "
            f"one records a checker answer. Either nothing has been checked, or "
            f"the line this reads has changed shape; both are 'unclassified', "
            f"which is not a pass."
        )
    unreadable = len(buckets["prose"]) + len(buckets["contentless"])
    head = "YES" if unreadable == 0 else "NO"
    firsts = {
        (a.strip('\\"').strip('"').split() or [""])[0]
        for a in buckets["prose"] + buckets["contentless"]
    }
    lines = [
        f"{head} — {total} recorded checker answer(s), {unreadable} unreadable",
        f"  contentless {len(buckets['contentless']):3d}  no words at all; a prompt "
        f"requirement has NO path to zero here",
        f"  prose       {len(buckets['prose']):3d}  words, but not a verdict first; "
        f"a prompt requirement reaches these",
        f"  verdict     {len(buckets['verdict']):3d}  first token is exactly YES or NO",
        f"  {len(firsts)} distinct first token(s) among the unreadable",
        "  numerator only: the driver logs an answer ONLY when it could not read "
        "one, so no rate can be taken from this",
    ]
    return unreadable == 0, "\n".join(lines)


def selftest() -> int:
    """The tool's own gate.

    ★ The property that matters is not "does it compute the right answer" — it
    is **does its output begin with a word the driver can read**, in both
    directions. A verdict tool that answers correctly in prose is the defect it
    was built for.
    """
    failures: list[str] = []
    ran = 0

    def expect(what: str, cond: bool) -> None:
        # ★ R1901 — the count is DERIVED. It used to be `12 - len(failures)`,
        # with the 12 written by hand here and again in the hook that greps this
        # line, so adding a case made both lie in the same direction. The same
        # defect this tool's docstring was carrying about the census number.
        nonlocal ran
        ran += 1
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

    # ── R1901: the census classifier ────────────────────────────────────────
    #
    # Every case here is a shape the driver actually recorded, not one invented
    # to exercise a branch — which is what makes the three buckets a reading of
    # the evidence rather than a taxonomy.
    # ★★★★★ THE EXTRACTOR, against VERBATIM driver lines. This is where this
    # round's bug was, and a fixture this file invented would not have found it:
    # the driver's sentence CONTINUES after the closing quote, and taking the
    # rest of the line made every answer look like prose.
    real_contentless = (
        'the checker answered \\"<br>\\", and nothing in that reply is a YES or '
        "a NO — fix its prompt or its program, because nothing here can turn that"
    )
    real_empty = (
        'the checker answered \\"\\", which is not YES or NO — fix its prompt or '
        "its program, because nothing here can turn that into a verdict"
    )
    expect("an answer is read up to its closing quote and no further",
           extract_answers(real_contentless) == ["<br>"])
    expect("an empty answer is read as empty, not as the driver's sentence",
           extract_answers(real_empty) == [""])
    expect("a line with no checker answer yields none",
           extract_answers("Reviewing --ReviewNone--> Priming") == [])
    expect("two answers in one text are read as two",
           len(extract_answers(real_contentless + "\n" + real_empty)) == 2)
    # ★ And the property the bug violated, stated directly: the driver's own
    # words about the answer must not become part of the answer.
    expect("the driver's sentence is not part of the answer",
           all("fix its prompt" not in a for a in extract_answers(real_contentless)))

    expect("a bare YES is readable", classify("YES") == "verdict")
    expect("a bare NO is readable", classify("NO") == "verdict")
    expect("a verdict with a dash after it is readable",
           classify("YES — R1900 is closed") == "verdict")
    expect("a lower-case yes is NOT readable, because the driver reads YES",
           classify("yes, the round is closed") == "prose")
    expect("a Verdict: label is prose, not a verdict", classify("Verdict: YES") == "prose")
    expect("a polite preamble is prose", classify("Permission to continue") == "prose")
    expect("the sentence that opened this debt is prose",
           classify("I've verified the code, tests, spec") == "prose")
    # ★★★★★ The two shapes a PROMPT REQUIREMENT CANNOT REACH. Both are on
    # record, and telling a checker to lead with a verdict does nothing for
    # either, because neither contains anything a verdict could lead.
    expect("an empty answer is contentless", classify("") == "contentless")
    expect("a bare line-break tag is contentless, though it has letters in it",
           classify("<br>") == "contentless")
    expect("markup around real words is still prose",
           classify("<br>Still checking") == "prose")
    expect("punctuation alone is contentless", classify("...") == "contentless")
    # Every answer lands in exactly one of three buckets and there is no fourth,
    # so nothing is silently uncounted — the rule this repository applies to any
    # gate with an escape hatch.
    expect(
        "every classification is one of exactly three",
        {classify(s) for s in ["YES", "no", "", "<br>", "The check passed"]}
        <= {"verdict", "prose", "contentless"},
    )

    # ★ A census that cannot see its source must REFUSE, never report zero.
    seen_none, said_none = census(ROOT / "tools" / "no-such-log-dir")
    expect("a census with no log to read answers NO", not seen_none)
    expect("and says it could not look", "could not look" in said_none)
    expect("a refusing census still leads with the verdict word", said_none.startswith("NO"))

    # `--line` is one line, and it is the verdict line.
    one = render(1, True, fake_ok).splitlines()[0]
    expect("the line form is a single line", "\n" not in one)
    expect("and it is the verdict", one.startswith("YES"))

    for line in failures:
        print(f"  FAIL {line}")
    # ★ `PASS` / `FAIL`, the form `phase_b_tally.py --selftest` already uses, so
    # the gate beside this greps a WORD rather than a number it has to be told
    # when to change.
    print(
        f"round_verdict selftest: {'PASS' if not failures else 'FAIL'} "
        f"({len(failures)} failure(s), {ran - len(failures)} of {ran} passed)"
    )
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("round", nargs="?", help="round token, e.g. R1891")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument(
        "--line",
        action="store_true",
        help="exactly one line: the verdict and nothing to choose between",
    )
    parser.add_argument(
        "--census",
        action="store_true",
        help="classify every checker answer the loop driver recorded",
    )
    parser.add_argument(
        "--from",
        dest="from_dir",
        help="where the driver's run logs are (default: $XDG_RUNTIME_DIR/loop)",
    )
    parser.add_argument(
        "--json", action="store_true", help="machine form; the verdict is still the first field"
    )
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if args.census:
        clean, said = census(Path(args.from_dir) if args.from_dir else None)
        print(said)
        return 0 if clean else 1

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
    elif args.line:
        print(render(round_no, closed, checks).splitlines()[0])
    else:
        print(render(round_no, closed, checks))
    return 0 if closed else 1


if __name__ == "__main__":
    sys.exit(main())
