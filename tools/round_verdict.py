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

⚠⚠ **R1906 — and it was not silent is only true of ONE of the two shapes the
driver records.** Measured against the driver's own logs at R1906, it writes
*two* different sentences when a check fails to verify a milestone: ``the
checker answered "…"`` — an answer it could not read — and ``the checker was
started and never answered: the wait ended <reason>``. Until R1906 this census
matched only the first, so the second was **not in the population at all**: not
a bucket, not a refusal, not an "unclassified" — absent. A count of checker
failures that cannot see a whole failure MODE is the escape-hatch shape this
repository's standing rules name, and the reason it went unnoticed for five
rounds is the ordinary one — the class that hit was the class the instrument
was blind to, so it never appeared in its own report.

⇒ ★★★★★ *An instrument built from the failures you have seen cannot count the
failure that stopped you seeing.* Both shapes are counted now, and they are kept
APART rather than summed into "failures", because the two prescriptions on
record reach different sets: telling a checker to lead with a verdict reaches an
answer with words in it, and reaches **neither** an answer with no words nor a
checker that never spoke.

⚠⚠ **R1963 — and "a checker that never spoke" is not one problem either.** The
driver writes, in the same sentence and after its own advice, what the PANE was
doing; R1906's regex stops at the wait reason, so this census threw that clause
away and reported all eighteen as a bucket nothing reaches. Measured against the
real logs: **8** were asked and displayed — the judge simply outran the bound,
and those are the only ones a faster judge reaches; **4** the driver records as
never having been ASKED at all, because the run ended between the typing and the
Enter, so *asking again* reaches them and no judge would have helped; **1**
never reached a pane a judge reads; **5** carry no clause and are
``unclassified``, which is reported and never folded into the other three.

⇒ ★★★★★ *The instrument was discarding the diagnosis its own source had already
written.* Not a blind spot in the regex's shape this time but in its length —
R1906 anchored it to stop at the reason word for a good reason (R1901's bug was
taking the rest of a line) and the cost of that care was everything after it.

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
import tempfile
import time
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


#: ★★★★★ R1968.1 — **what this judge COSTS, measured rather than guessed at.**
#:
#: The driver's own sentence when a check verifies nothing is *"the wait ended
#: NotYet — give it longer, or a faster judge"*. That is two prescriptions and
#: it prints them together, unconditionally, whichever applies — measured at
#: R1968.1 over every record in the driver's log directory, the clause that
#: would say WHICH is absent from the great majority: 335 of the tails after
#: that advice are empty. So an operator reading it cannot tell a judge that
#: was too slow from one that was never asked, and the repository's answer to
#: "the reason is prose" is to make the reason a number somebody can read.
#:
#: `remote` is separated from `total` because it is the one call that leaves the
#: machine, and it is where the time is: 2.32s of a 3.11s answer, measured
#: three times. A reader deciding between the two prescriptions needs that
#: split — a judge that is 75% one SSH handshake is not made faster by anything
#: this tool does to its other five predicates.
#:
#: ⚠ NOT reported by `--line`, which is EXACTLY one line by contract and whose
#: whole purpose is that a reader has nothing to choose between. The selftest
#: holds that.
TIMING: dict[str, float] = {}


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
    # ★ R1968.1 — timed, because this is the only call that leaves the machine
    # and a reader told to find "a faster judge" needs to know that.
    began = time.monotonic()
    status, out = run("git", "ls-remote", "origin", "main")
    TIMING["remote"] = time.monotonic() - began
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


def _porcelain() -> tuple[list[str], list[str], str | None]:
    """The working tree, split into TRACKED changes and UNTRACKED paths.

    ★★★★★ R1951 — **this function's caller said "tracked" in three places and
    counted untracked paths anyway.** Its name was `check_tree_clean`, its
    module docstring said *"no uncommitted tracked changes left behind"*, and
    its own passing message said *"no uncommitted tracked change"* — while the
    code counted every line `git status --porcelain` printed, `??` entries
    included. Three statements of a property and no code performing it, which
    is the class this repository has been caught by four rounds running.

    The `??` prefix is the whole of the distinction and it is porcelain's own,
    so this is a split rather than a heuristic.
    """
    # `raw`, because porcelain's first two columns ARE data — see `run`.
    status, out = run("git", "status", "--porcelain", raw=True)
    if status != 0:
        return [], [], f"git status could not run: {out[:200]}"
    tracked, untracked = split_porcelain(out)
    return tracked, untracked, None


def split_porcelain(text: str) -> tuple[list[str], list[str]]:
    """Porcelain output → `(tracked paths, untracked paths)`.

    Pure, so [`selftest`] can hold it to the distinction rather than trusting
    that the caller got it right — which is the whole reason R1951 found the
    defect above by reading rather than by running.
    """
    tracked: list[str] = []
    untracked: list[str] = []
    for line in text.splitlines():
        if not line.strip():
            continue
        (untracked if line.startswith("??") else tracked).append(line[3:])
    return tracked, untracked


def check_tree_clean() -> Check:
    """No TRACKED file is left uncommitted.

    A tracked file has been committed before, so an edit to one is this round's
    to own — and leaving it behind means the tree and `origin/main` disagree
    about code that is already published.
    """
    tracked, _, why = _porcelain()
    if why:
        return Check("tree", False, why)
    return Check(
        "tree",
        not tracked,
        "no uncommitted tracked change"
        if not tracked
        else f"{len(tracked)} uncommitted tracked path(s): "
        f"{', '.join(tracked[:4])}",
    )


def check_untracked() -> Check:
    """Untracked paths, NAMED — and deliberately not a veto.

    # Why this cannot decide the verdict

    ★★★★★ Measured at R1951: this repository's worktree is SHARED — its own
    memory records that a concurrent loop run commits into it — and
    `crates/pinion-node-graph/src/review.rs` had been sitting in it untracked,
    present in **no commit on any branch**, since before R1948. Folded into the
    tracked check it turned R1948, R1949, R1950 and R1951 all NO, every one of
    them for a file the round in question never touched and must not delete
    (this project's standing rule bans `git add -A` for exactly this reason).
    ⇒ **there was no path by which any of those rounds could make that value
    zero**, which is the shape a terminating predicate must never have: a
    population carrying a role that cannot reach the state being demanded.

    # Why it is still reported

    Because *not a veto* is not *not looked at*. The paths are named here, and
    [`render`] prints this line whether or not it passes, so an untracked file
    a round did leave behind is in front of whoever reads the verdict. What is
    given up is the ability to REFUSE over it, and that ability was never real
    in a shared tree.

    ⚠ The property this does not cover: a round whose committed code needs an
    untracked file. That is caught where it is actually visible — the clone CI
    builds does not have the file, so the build fails.
    """
    _, untracked, why = _porcelain()
    if why:
        return Check("untracked", False, why)
    return Check(
        "untracked",
        True,
        "no untracked path"
        if not untracked
        else f"{len(untracked)} untracked path(s), not judged (shared worktree): "
        f"{', '.join(untracked[:4])}",
    )


def verdict(round_no: int) -> tuple[bool, list[Check]]:
    began = time.monotonic()
    checks = [
        check_commit(round_no),
        check_ledger(round_no),
        check_tsv(round_no),
        check_published(round_no),
        check_debt_snapshot(),
        check_tree_clean(),
        # ★ R1951 — reported, never a veto. See `check_untracked` for the
        # measurement that made it one and for what that costs.
        check_untracked(),
    ]
    TIMING["total"] = time.monotonic() - began
    return all(c.ok for c in checks), checks


def cost_line() -> str | None:
    """What this answer cost, or `None` when nothing was timed.

    ⚠ `None` rather than zeroes: a cost of `0.00s` would be a measurement, and
    a run that never reached the remote probe (a round with no commit) made no
    measurement at all. This tool's own rule about a check that could not look
    applies to its own instrument.
    """
    total = TIMING.get("total")
    if total is None:
        return None
    remote = TIMING.get("remote")
    if remote is None:
        return f"  cost: {total:.2f}s, none of it asking origin (never reached that check)"
    share = 100.0 * remote / total if total > 0 else 0.0
    return (
        f"  cost: {total:.2f}s total, {remote:.2f}s ({share:.0f}%) asking origin "
        "— the one call that leaves this machine"
    )


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
    # ★ R1968.1 — LAST, and never first: the first word is the verdict and
    # nothing may precede it. `--line` takes `splitlines()[0]`, so this cannot
    # reach the one-line form however long it grows.
    cost = cost_line()
    if cost is not None:
        lines.append(cost)
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

#: The driver's OTHER sentence: the checker was asked and produced nothing at
#: all, so there is no answer to read, readable or otherwise. The capture is the
#: wait's own reason word, because "it is still thinking" and "the run ended
#: under it" are different problems with different repairs.
#:
#: ★★★★★ R1906 — THE SHAPE THIS CENSUS COULD NOT SEE, and the one that produced
#: the failure that sent this round here. `CHECKER_ANSWERED` needs the words
#: "the checker answered", which this line does not contain, so every such
#: record fell outside the population silently — the census reported its answer
#: buckets and said nothing about the records it had never matched.
#:
#: Deliberately anchored on the driver's phrasing UP TO the reason word and no
#: further: the sentence continues with advice ("give it longer, or a faster
#: judge") that is the driver's, not the checker's, and R1901's bug was exactly
#: taking the rest of a line.
CHECKER_UNANSWERED = re.compile(
    r"the checker was started and never answered: the wait ended (\w+)"
)

#: ★★★★★ R1963 — **what the driver says about the PANE**, on a check that never
#: answered, and the phrase it says it with.
#:
#: The wait reason above is *how the wait ended*; this is *what was happening at
#: the other end*, and the driver has been writing it in the same sentence all
#: along — after its own advice, which is why R1906's regex, deliberately
#: anchored to stop at the reason word, never reached it.
#:
#: ⚠⚠ It matters because the census's own line says **NEITHER prescription
#: reaches these**, and measured against the real logs that is true of only
#: eight of the eighteen. Four of them the driver records as never having been
#: ASKED — the run ended between the typing and the Enter — so *asking again*
#: reaches those, and one never reached a pane a judge reads. Summing four
#: problems into one number and declaring nothing reaches any of them is the
#: escape-hatch shape this repository refuses, met inside the instrument built
#: to remove it one level up.
#:
#: Order is longest-evidence-first only in the sense that the phrases are
#: disjoint on the driver's real lines; the selftest asserts that a line
#: carrying one is not read as another.
UNANSWERED_CAUSE: tuple[tuple[str, str], ...] = (
    # Asked, delivered, and displayed — the judge simply had not finished. This
    # is the ONLY cause a faster judge or a longer bound reaches, and
    # `round_verdict.py --line` is the faster judge that already exists.
    ("displayed", "the pane painted the prompt back"),
    # Nothing was ever asked. Re-asking reaches these, and no change to the
    # judge would have helped.
    ("never asked", "the run ended between the typing and the Enter"),
    # The prompt never reached a screen a judge reads.
    ("off screen", "NOWHERE ON THAT SCREEN"),
)

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


def extract_unanswered(text: str) -> list[str]:
    """Every check the driver recorded as never having answered, by wait reason.

    Separate from [`extract_answers`] and not folded into it, because these are
    not answers: there is nothing to classify, and a bucket for them inside the
    answer taxonomy would say a checker that never spoke had said something
    unreadable. The two are disjoint on the driver's real lines, which the
    selftest asserts in both directions — the discriminating property, since a
    regex that matched both would report the same total while destroying the
    distinction this round exists to make.
    """
    return [m.group(1) for m in CHECKER_UNANSWERED.finditer(text)]


def unanswered_cause(text: str, at: int) -> str:
    """★ R1963 — which of [`UNANSWERED_CAUSE`] the driver named for the record
    that starts at `at`, or ``unclassified`` when it named none.

    Read from the record's own LINE and no further, because the driver writes
    one transition per line and a phrase from the next record would be a cause
    attributed to the wrong check — R1901's bug, which was taking the rest of a
    line, one step further out.

    ⚠ ``unclassified`` is a real answer and is never folded into a named cause.
    Measured on the real logs, five of eighteen records carry no clause at all;
    a reader told they were "displayed" would be told something the driver never
    said. There IS a path to zero — the driver writes a clause on the other
    thirteen — so the bucket is a finding rather than a permanent excuse.
    """
    end = text.find("\n", at)
    line = text[at:] if end == -1 else text[at:end]
    for name, phrase in UNANSWERED_CAUSE:
        if phrase in line:
            return name
    return "unclassified"


def extract_unanswered_causes(text: str) -> list[str]:
    """The cause the driver named for every unanswered record, in order."""
    return [
        unanswered_cause(text, m.end()) for m in CHECKER_UNANSWERED.finditer(text)
    ]


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
    waits: list[str] = []
    causes: list[str] = []
    for log in logs:
        text = log.read_text(errors="replace")
        for answer in extract_answers(text):
            buckets[classify(answer)].append(answer)
        waits.extend(extract_unanswered(text))
        causes.extend(extract_unanswered_causes(text))
    answered = sum(len(v) for v in buckets.values())
    total = answered + len(waits)
    if total == 0:
        return False, (
            f"NO — could not look: {len(logs)} run log(s) under {where} and not "
            f"one records a checker answer OR a check that never answered. "
            f"Either nothing has been checked, or a line this reads has changed "
            f"shape; both are 'unclassified', which is not a pass."
        )
    unreadable = len(buckets["prose"]) + len(buckets["contentless"])
    unverified = unreadable + len(waits)
    head = "YES" if unverified == 0 else "NO"
    firsts = {
        (a.strip('\\"').strip('"').split() or [""])[0]
        for a in buckets["prose"] + buckets["contentless"]
    }
    # The wait reasons, most common first, because "it is still thinking" and
    # "the run ended under it" are different problems: one wants a longer bound
    # or a faster judge, the other wants the check to start earlier.
    why = ", ".join(
        f"{reason} {waits.count(reason)}"
        for reason in sorted(set(waits), key=lambda r: (-waits.count(r), r))
    )
    lines = [
        f"{head} — {total} recorded check(s) that verified nothing: "
        f"{answered} answered unreadably, {len(waits)} never answered",
        f"  contentless {len(buckets['contentless']):3d}  no words at all; a prompt "
        f"requirement has NO path to zero here",
        f"  prose       {len(buckets['prose']):3d}  words, but not a verdict first; "
        f"a prompt requirement reaches these",
        f"  verdict     {len(buckets['verdict']):3d}  first token is exactly YES or NO",
        # ★★★★★ R1906 — the shape this census was blind to. Its own line rather
        # than a share of `prose`, because a checker that never spoke has said
        # nothing to lead with a verdict, so NEITHER recorded prescription — the
        # prompt half or the parser half — reaches it. Naming it as a fourth
        # bucket inside the answer taxonomy would have hidden that.
        # ★★★★★ R1963 — it used to end at "NEITHER prescription reaches these",
        # and measured against the driver's own words that is true of only the
        # `displayed` share. The driver names what the pane was doing; the four
        # lines below are that, read rather than discarded, because the four
        # causes have four different repairs and only one of them is a judge.
        f"  unanswered  {len(waits):3d}  the checker never spoke"
        + (f" — {why}" if why else ""),
        f"      displayed    {causes.count('displayed'):3d}  asked and ON the pane; "
        f"only a faster judge or a longer bound reaches these",
        f"      never asked  {causes.count('never asked'):3d}  the run ended before "
        f"the Enter, so nothing was asked — ASKING AGAIN reaches these",
        f"      off screen   {causes.count('off screen'):3d}  the prompt never "
        f"reached a pane a judge reads",
        f"      unclassified {causes.count('unclassified'):3d}  the driver named no "
        f"cause; NOT folded into any of the three above",
        f"  {len(firsts)} distinct first token(s) among the unreadable",
        "  numerator only: the driver records a check ONLY when it failed to "
        "verify, so no rate can be taken from this",
    ]
    return unverified == 0, "\n".join(lines)


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

    # ★★★★★ R1951 — the tracked / untracked split, which three sentences of
    # this file claimed and no line of it performed. Verbatim porcelain,
    # including the leading-space form R1893 was caught by.
    tracked, untracked = split_porcelain(
        " M crates/pinion-core/src/lib.rs\n"
        "?? crates/pinion-node-graph/src/review.rs\n"
        "A  tools/new.py\n"
    )
    expect(
        "an untracked path is not a tracked change",
        tracked == ["crates/pinion-core/src/lib.rs", "tools/new.py"],
    )
    expect(
        "an untracked path is kept, and named",
        untracked == ["crates/pinion-node-graph/src/review.rs"],
    )
    expect(
        "the leading space of a ` M ` line is not eaten",
        tracked[0].startswith("crates/"),
    )
    # ★ And the property the split exists for: a tree holding NOTHING BUT
    # untracked paths leaves the verdict alone. Asserted on the Check objects
    # rather than by running git, so it holds on any machine.
    expect(
        "untracked paths never veto a verdict",
        Check("untracked", True, "…").ok,
    )
    expect(
        "an untracked-only tree is a clean tracked tree",
        split_porcelain("?? a.rs\n?? b.rs\n")[0] == [],
    )

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

    # ── R1906: the shape that was outside the population ────────────────────
    #
    # ★★★★★ Verbatim driver lines again, and for a sharper reason than R1901's:
    # this whole class was invisible because nothing here had ever SEEN one. A
    # fixture invented from the docstring would have described the failures
    # already known, which is exactly how the blind spot survived.
    real_unanswered = (
        'it said: \\"the checker was started and never answered: the wait ended '
        'NotYet — give it longer, or a faster judge\\" — it was shown the '
        "agent's own account of this turn"
    )
    real_run_ended = (
        'it said: \\"the checker was started and never answered: the wait ended '
        'RunEnded — give it longer, or a faster judge\\" — the run ended between '
        "the typing and the Enter, so nothing was asked"
    )
    expect("a check that never answered is read, with its wait reason",
           extract_unanswered(real_unanswered) == ["NotYet"])
    expect("and a run that ended under the wait is a different reason",
           extract_unanswered(real_run_ended) == ["RunEnded"])
    expect("the driver's advice is not part of the reason",
           all("give it longer" not in r for r in extract_unanswered(real_unanswered)))
    # ★★★★★ THE DISCRIMINATING PROPERTY, both directions. A regex matching both
    # shapes would keep the total right and destroy the distinction, which is
    # the failure this round is repairing rather than the one it could cause.
    expect("an unanswered check is NOT read as an answer",
           extract_answers(real_unanswered) == [])
    expect("an unreadable answer is NOT read as unanswered",
           extract_unanswered(real_contentless) == [])
    expect("a line with neither shape yields neither",
           extract_unanswered("Reviewing --ReviewNone--> Priming") == []
           and extract_answers("Reviewing --ReviewNone--> Priming") == [])

    # ★★★★★ R1963 — the CAUSE the driver named, on lines taken verbatim from
    # its logs. Verbatim for R1906's reason, one level down: a fixture written
    # from the docstring would carry the causes already known, and the whole
    # point is that the driver was writing one this census had never read.
    real_displayed = (
        'it said: \\"the checker was started and never answered: the wait ended '
        'NotYet — give it longer, or a faster judge\\" — it was shown the '
        "agent's own account of this turn — the pane painted the prompt back, "
        "so it is on that screen"
    )
    real_off_screen = (
        'it said: \\"the checker was started and never answered: the wait ended '
        'NotYet — give it longer, or a faster judge\\" — the agent itself named '
        "this question as the one it received and the prompt is NOWHERE ON THAT "
        "SCREEN — its composer folded the paste away"
    )
    expect("a check that was asked and displayed is named as displayed",
           extract_unanswered_causes(real_displayed) == ["displayed"])
    expect("a run that ended before the Enter is named as never asked",
           extract_unanswered_causes(real_run_ended) == ["never asked"])
    expect("a prompt that never reached a screen is named as off screen",
           extract_unanswered_causes(real_off_screen) == ["off screen"])
    # ★★★★★ THE ESCAPE HATCH, closed. A driver line with no clause must be
    # `unclassified` and must NOT be quietly attributed to the commonest cause —
    # measured, five of the eighteen real records carry no clause, so the wrong
    # answer here would be five records of invented evidence.
    expect("a record the driver said nothing about is unclassified, not guessed",
           extract_unanswered_causes(real_unanswered) == ["unclassified"])
    expect("an unknown clause is unclassified rather than matched to a cause",
           extract_unanswered_causes(
               real_unanswered + " — the pane was doing something new"
           ) == ["unclassified"])
    # ★ And a cause is read from the record's OWN line. Two records on two lines
    # keep their own causes; a phrase from the next line is a cause attributed
    # to the wrong check, which is R1901's bug one step further out.
    expect("a cause does not leak across a line break",
           extract_unanswered_causes(real_unanswered + "\n" + real_displayed)
           == ["unclassified", "displayed"])
    expect("every cause name is one the report prints",
           set(name for name, _ in UNANSWERED_CAUSE)
           == {"displayed", "never asked", "off screen"})

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

    # ★★★★★ R1906 — the AGGREGATION, not only the extractors. The blind spot was
    # here: both regexes could be perfect and the census still report a
    # population that omits one of them. Driven over a real log directory shape,
    # with one log holding one of each, so the reported line is the thing under
    # test rather than a count computed twice.
    with tempfile.TemporaryDirectory() as tmp:
        (Path(tmp) / "run1.log").write_text(
            real_contentless + "\n" + real_unanswered + "\n" + real_run_ended + "\n"
        )
        seen_both, said_both = census(Path(tmp))
        expect("a census that sees an unanswered check does not report YES", not seen_both)
        expect("it counts BOTH shapes in the population",
               "3 recorded check(s)" in said_both)
        expect("and keeps them apart rather than summing them",
               "1 answered unreadably, 2 never answered" in said_both)
        expect("the unanswered bucket is reported on its own line",
               "unanswered    2" in said_both)
        expect("with the wait reasons broken out, commonest first",
               "NotYet 1, RunEnded 1" in said_both)
        # ★★★★★ R1963 — this case asserted the sentence *NEITHER prescription
        # reaches them*, and the sentence turned out to be FALSE. Measured
        # against the driver's own logs: of eighteen unanswered records it
        # names four as never having been ASKED, which *asking again* reaches,
        # and one as never having reached a pane. So the line is split by the
        # driver's own diagnosis now, and this case asserts the split instead of
        # the claim it disproved. The fixture holds one `never asked` (its
        # verbatim line carries that clause) and one with no clause at all.
        expect("the unanswered line no longer claims nothing reaches them",
               "NEITHER prescription reaches" not in said_both)
        expect("the cause the driver named is reported",
               "never asked    1" in said_both)
        expect("and a record it named no cause for is reported as unclassified",
               "unclassified   1" in said_both)
        expect("a cause the fixture has none of reads zero rather than vanishing",
               "displayed      0" in said_both and "off screen     0" in said_both)
    # A directory whose logs record neither shape must REFUSE, not report a
    # clean bill — the same rule as an unreadable source, one level in.
    with tempfile.TemporaryDirectory() as tmp:
        (Path(tmp) / "run1.log").write_text("Reviewing --ReviewNone--> Priming\n")
        seen_quiet, said_quiet = census(Path(tmp))
        expect("a log recording neither shape is a refusal", not seen_quiet)
        expect("and it says so as 'could not look'", "could not look" in said_quiet)

    # `--line` is one line, and it is the verdict line.
    one = render(1, True, fake_ok).splitlines()[0]
    expect("the line form is a single line", "\n" not in one)
    expect("and it is the verdict", one.startswith("YES"))

    # ★★★★★ R1968.1 — the cost line is REPORTED and never LEADS, and a run that
    # measured nothing says nothing rather than `0.00s`.
    #
    # ⚠ This asserts against `render`'s output rather than timing anything, so
    # it cannot go flaky on a slow host — a timing assertion in a gate is the
    # shape [[zero-flake-policy]] refuses. What is held is the ARRANGEMENT:
    # where the number may appear, and that its absence is sayable.
    TIMING.clear()
    expect("a run that timed nothing offers no cost line", cost_line() is None)
    expect("and the report then carries none",
           "cost:" not in render(1, True, fake_ok))
    TIMING["total"] = 3.11
    TIMING["remote"] = 2.32
    priced = render(1, True, fake_ok)
    expect("a timed run states its cost", "cost: 3.11s total" in priced)
    expect("and attributes the share that left the machine",
           "2.32s (75%) asking origin" in priced)
    expect("the cost is the LAST line, so the verdict is still first",
           priced.splitlines()[-1].lstrip().startswith("cost:")
           and priced.splitlines()[0].startswith("YES"))
    expect("★ and it cannot reach the one-line form",
           "cost:" not in priced.splitlines()[0])
    del TIMING["remote"]
    expect("a total with no remote reading says so rather than reading zero",
           "none of it asking origin" in (cost_line() or ""))
    TIMING.clear()

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
                    # ★ R1968.1 — seconds, so a driver choosing between "give it
                    # longer" and "a faster judge" reads a number instead of
                    # both. `remote_s` is absent when that check was not reached.
                    "cost_s": {name: round(v, 3) for name, v in sorted(TIMING.items())},
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
