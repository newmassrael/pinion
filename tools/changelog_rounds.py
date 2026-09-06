#!/usr/bin/env python3
"""The round ledger's reader, and the gate that keeps it from stopping (R1746).

`CLAUDE.md` says every non-trivial decision lives in the Mnemosyne atomic store,
appended once per round with `mnemosyne-cli append-changelog-entry`. Nothing has
ever checked that it happened. `docs/phase-b-rounds.tsv` — the other once-per-
round artifact — *is* checked (`phase_b_tally.py` prints UNDECLARED for a round
git has and the ledger lacks), and the difference is measurable: at R1746 the TSV
had a row for every round and the changelog had none for R1743, R1744 or R1745.

## Why this needed a READER before it could have a gate

R1744 noticed the gap and mis-sized it by an order of magnitude, writing down a
34-round outage that did not exist. That was not carelessness — **there was no
correct way to ask.** Two readers were available and both answer wrongly:

* a one-liner matching `Round (\\d+)` sees only one of the store's three key
  forms and reports the newest entry as `Round 1708.4`, because the keys shaped
  `R1742` — the majority of them, and the form every recent round uses — are
  invisible to it;
* `mnemosyne-cli query --list-changelog` documents "round order, oldest first"
  and `--limit N` as "the newest N". Measured at R1746 against this store, it
  groups by key FORM and then sorts lexicographically inside each group, so its
  newest fourteen end `R999, SCE-RFC-002-004, round-14, round-15` — the
  documented tool answers *Round 15* for "the latest entry", off by 1,730.

So the store's own audit trail could not answer "which round did you last
record", and a question nobody can ask is a rule nobody can keep. This module is
that answer, and the numbers below are derived from the store every run — no
count here is written down in prose.

## What it refuses

`--check` fails when a round git knows about, above `ENTRY_FLOOR`, has no
changelog entry — **except the newest round, which is the round in progress.**
That exemption is what makes the gate usable rather than merely strict: a round
legitimately pushes before it closes (R1728.1, R1735.1), and its entry is
written at close. The bound is still tight: the round AFTER a skipped one cannot
be published, so a miss surfaces one round later instead of three (or ninety-
four — see the floor).

`--check` also fails on a key whose form this module cannot parse and that
`NON_ROUND_KEYS` does not name. An unrecognised key is a round this gate would
otherwise count as absent, or an entry it would credit to the wrong round; a
silent skip is the failure mode the whole file is about, so a new form has to be
declared rather than tolerated.

## What it does NOT check

* **Whether the entry says anything.** One sentence satisfies it. This gate
  answers "did the round record its decisions somewhere an audit can find them",
  not "are they worth reading" — and a gate that tried to judge the second would
  be a gate nobody could pass on purpose.
* **A round git cannot see.** The population is commit subjects, via the same
  parse the Phase B tally uses, so a round whose commits do not follow
  `type(scope): R<N> …` is invisible to BOTH gates rather than to one.

  ★★★★★ R1828 — **this bullet carried a prose stamp and the stamp was false.**
  It said *"Measured at R1746: no such subject exists in this history"*, which
  is true of the CONVENTIONAL-COMMIT half of the shape and false of the `R<N>`
  half, and the sentence names both. Re-measured with this module's own shared
  parse, over the same history: **0** subjects fail `type(scope):`, **0** carry
  a `!` breaking marker — and **61 of 2456 carry no round number at all**,
  `aefbe934` (the commit that prompted this round) among them. The two clauses
  were welded into one sentence, so the true half vouched for the false half.

  The stamp is therefore replaced by a **printed line**, not by a better
  number: `roundless_subjects` is counted and reported on every run, the way
  `target/`'s size is. A count in prose starts rotting the moment it is
  written; a count the tool prints cannot.

  ⚠ **And what the line reports is not a defect.** A commit that closes no
  round legitimately carries no round number — `COMMIT_FORMAT.md` recommends
  `R<N>` and does not require it, and vendor bumps and CI fixes are the honest
  majority of those 61. Which of them was *a round that forgot its number* is
  not decidable from a subject, and this module does not pretend otherwise: it
  reports the population and leaves the judgment to a reader. `aefbe934` is the
  case that shows the distinction is real — it added a gate library, three hook
  call sites and its tests, and needed an entry.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

# The round population is SHARED with the Phase B tally on purpose. Two gates
# that each parse round numbers out of commit subjects can disagree, and then one
# demands an entry for a round the other cannot see — so the parse has exactly
# one implementation and this file imports it rather than restating it. The floor
# is NOT shared: the tally's is R1519 (when it started), this one's is older.
from phase_b_tally import rounds_in  # noqa: E402  (path set above)

ROOT = Path(__file__).resolve().parent.parent
STORE = ROOT / "docs" / ".atomic" / "workspace.atomic.json"

#: Rounds at or below this floor are grandfathered, and the report says how many
#: of them are missing rather than letting the floor hide them.
#:
#: R1359 is where the last historic outage ends: R1266–R1359 landed with almost
#: no entries. Measured at R1746, 101 rounds in all of history have none and 86
#: of them are in that block; the live count is printed on every run, so this
#: stamp cannot quietly become the only number anyone reads. Nothing about that
#: period can be reconstructed honestly now, so it is declared instead of
#: quietly excluded — and the floor being *below* the tally's R1519 is
#: deliberate, because the ledger's own claim ("all non-trivial decisions live
#: here") is older than the tally.
ENTRY_FLOOR = 1359

#: Changelog keys that name no round at all. Both are RFC records rather than
#: round closures, which is why they have no round to be missing from.
#:
#: This is a pin, not a filter: `--check` fails on any other unparseable key.
#: "An exclusion is a claim" — the same rule the reference census runs under.
NON_ROUND_KEYS = frozenset({"SCE-RFC-002-004", "SCE-RFC-002-structural"})

#: Every key form the store actually holds, in one expression: an optional
#: `Round `/`Round-`/`R`/`r` prefix, the round number, then any sub-round label
#: whatsoever. The label shapes in the store are `.4`, `.2a`, `b`, `.109.0`,
#: `.D.1`, `.1.b.1.tui`, `.A` — enumerating them was never going to hold, so the
#: number is what is parsed and the rest is only required to exist.
KEY_ROUND = re.compile(r"(?:[Rr]ound[ -]|[Rr])?(\d+)")


def round_of(key: str) -> int | None:
    """The round `key` records, or None if it names no round.

    `.match` anchors at the start, which is the whole discrimination: `R1742` and
    `Round 1708.4` and `round-14` and a bare `408` all yield their number, while
    `SCE-RFC-002-004` yields None because its digits are not at the head.
    """
    m = KEY_ROUND.match(key)
    return int(m.group(1)) if m else None


def store_keys(path: Path = STORE) -> list[str]:
    """The changelog entry keys, read from the store.

    Read direct rather than through `mnemosyne-cli query --list-changelog`: the
    CLI's ordering cannot answer this question (see the module docstring), and
    the keys are all that is wanted. `json.load` of the 14 MB store measured
    0.10s at R1746 — cheaper than the tally's tree walk, which runs on the same
    hook. Reading is not the operation `CLAUDE.md` forbids; every WRITE still
    goes through the typed primitives.
    """
    return list(json.loads(path.read_text(encoding="utf-8"))["changelog_entries"])


def git_subjects() -> str:
    """Every non-merge commit subject, newest first.

    Split out at R1828 so the two things read off it — the round population and
    the count of subjects naming no round — come from ONE `git log` rather than
    two that could disagree about the history they read.
    """
    return subprocess.run(
        ["git", "log", "--format=%s", "--no-merges"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def git_rounds(subjects: str | None = None) -> list[int]:
    """Rounds git knows about. Git is the authority on which rounds EXIST — if
    the ledger were also the census, a round that never appended would be
    invisible instead of reported."""
    return rounds_in(git_subjects() if subjects is None else subjects)


def roundless_subjects(subjects: str) -> list[str]:
    """Commit subjects the shared parse yields no round for. Pure in `subjects`.

    ★★★★★ R1828 — the measurement that replaced a prose stamp in the docstring
    above. It is NOT a finding on its own: a commit closing no round carries no
    round number legitimately. It exists so the number can never be *asserted*
    from memory again, and so a reader who wants to know whether a substantive
    change slipped past both round gates has a list to look at rather than a
    sentence to trust.

    Uses `rounds_in` — the parse the gates actually run — rather than a regex of
    its own, because a second spelling of "does this subject name a round" is
    exactly the drift this module's own header warns about one function up.
    """
    return [line for line in subjects.splitlines() if line.strip() and not rounds_in(line)]


class Audit:
    """What the two populations say about each other. Pure in its inputs, so the
    selftest drives it with fixtures rather than with the repository."""

    def __init__(self, keys: list[str], rounds: list[int], floor: int = ENTRY_FLOOR):
        self.floor = floor
        self.by_round: dict[int, list[str]] = {}
        self.non_round: list[str] = []
        self.unknown: list[str] = []
        for key in keys:
            rnd = round_of(key)
            if rnd is None:
                (self.non_round if key in NON_ROUND_KEYS else self.unknown).append(key)
            else:
                self.by_round.setdefault(rnd, []).append(key)

        self.recorded = sorted(self.by_round)
        self.last_recorded = self.recorded[-1] if self.recorded else None
        self.git = sorted(set(rounds))
        # The newest round git has is the one in progress: its commits exist and
        # its entry is written at close. Exempt from refusal, reported instead.
        self.in_progress = self.git[-1] if self.git else None
        missing = [r for r in self.git if r not in self.by_round]
        self.below_floor = [r for r in missing if r <= floor]
        above = [r for r in missing if r > floor]
        self.in_progress_missing = self.in_progress in above
        self.missing = [r for r in above if r != self.in_progress]

    def ok(self) -> bool:
        return not self.missing and not self.unknown


def report(audit: Audit) -> None:
    entries = sum(len(v) for v in audit.by_round.values())
    print(
        f"changelog: {entries} entries over {len(audit.recorded)} rounds; "
        f"last recorded R{audit.last_recorded}"
    )
    print(
        f"git: {len(audit.git)} rounds; newest R{audit.in_progress}"
        + (" (in progress)" if audit.in_progress_missing else "")
    )

    if audit.non_round:
        print(f"non-round entries (declared): {', '.join(sorted(audit.non_round))}")

    if audit.below_floor:
        # Said out loud every run. A floor that reports nothing is a floor that
        # hides an outage, which is how R1266-R1359 stayed unmentioned until a
        # round went looking for a different gap.
        print(
            f"below the R{audit.floor} floor: {len(audit.below_floor)} round(s) have "
            f"no entry (grandfathered, not reconstructable)"
        )

    if audit.unknown:
        print(
            f"\nUNDECLARED KEY FORM — {len(audit.unknown)} changelog key(s) name "
            f"no round this reader understands. Teach `KEY_ROUND` the form, or "
            f"name the key in `NON_ROUND_KEYS`; a key this gate cannot read is a "
            f"round it silently miscounts:"
        )
        for key in sorted(audit.unknown):
            print(f"  {key!r}")

    if audit.missing:
        print(
            f"\nUNRECORDED — {len(audit.missing)} round(s) landed in git with no "
            f"changelog entry. Append one with `mnemosyne-cli "
            f"append-changelog-entry --entry-id R<NNN> ...`; until then that "
            f"round's decisions are in no audit trail:"
        )
        for rnd in audit.missing:
            print(f"  R{rnd}")

    if audit.in_progress_missing:
        print(
            f"\nR{audit.in_progress} has no entry yet — the round in progress. "
            f"Not refused; it is refused from the next round's push onward."
        )


def selftest() -> int:
    """Every assertion here is a reader that was actually wrong about this store.

    The fixtures are built to DISCRIMINATE: each one contains the pair of keys a
    broken reader confuses, because a fixture both readers answer the same way
    tests nothing (the standing lesson of R1722, R1727, R1728 and R1742).
    """
    failures: list[str] = []

    def check(name: str, cond: bool) -> None:
        if not cond:
            failures.append(name)

    # --- 1. Every key form in the store, including the ones that hid the gap ---
    check("Round-form key parses", round_of("Round 1708.4") == 1708)
    check("R-form key parses", round_of("R1742") == 1742)
    check("bare key parses", round_of("408") == 408)
    check("lowercase round- key parses", round_of("round-14") == 14)
    check("letter sub-round parses", round_of("R1163b") == 1163)
    check("deep sub-round parses", round_of("R56.1.b.1.tui") == 56)
    check("letter sub-round after dot parses", round_of("R670.A") == 670)
    check("RFC key names no round", round_of("SCE-RFC-002-004") is None)
    check("a word starting with R names no round", round_of("Rewrite-3") is None)

    # --- 1b. R1828: the subjects that name no round -------------------------
    #
    # Discriminating on purpose, per this function's own rule: the fixture holds
    # a round subject, a conventional subject with no round, and a subject whose
    # `R` is not at the head of the message — because a reader that split on
    # "contains R<digits>" would put the last one in the wrong bin, and that is
    # the reader a hand-rolled regex here would have been.
    fixture = "\n".join(
        [
            "feat(app): R1827 a reply names the message it answers",
            "feat(hooks): gate the identity a commit and a push may carry",
            "chore(vendor): bump the SCE pin to a80b06d, 19 upstream fixes",
            "docs(meta): R1163b cross-window changelog",
            "fix(core): the R2 rail is not a round number",
        ]
    )
    roundless = roundless_subjects(fixture)
    check(
        "a subject with no round number is counted",
        "feat(hooks): gate the identity a commit and a push may carry" in roundless,
    )
    check(
        "a subject that names a round is not",
        "feat(app): R1827 a reply names the message it answers" not in roundless,
    )
    check(
        "a sub-round subject is not counted either",
        "docs(meta): R1163b cross-window changelog" not in roundless,
    )
    check(
        "an R inside the message is not a round number",
        "fix(core): the R2 rail is not a round number" in roundless,
    )
    check("the fixture's roundless count is exactly three", len(roundless) == 3)
    check("blank lines are not subjects", roundless_subjects("\n\n  \n") == [])
    # The population and the round list partition the subjects — which is what
    # makes the printed line readable beside `git: N rounds` rather than a third
    # number nobody can reconcile with the other two.
    check(
        "roundless and named rounds partition the fixture",
        len(roundless) + len({s for s in fixture.splitlines() if rounds_in(s)}) == 5,
    )

    # --- 2. The reader that produced the phantom 34-round outage -------------
    #
    # R1744's evidence block matched `Round (\d+)` only. On a store holding both
    # forms it answers the older one, and the fixture is chosen so the two
    # readers cannot agree: the newest entry is R-form, the newest Round-form
    # entry is older.
    mixed = ["Round 1708.4", "R1742", "R1741"]
    form_blind = max(
        (int(m.group(1)) for k in mixed if (m := re.fullmatch(r"Round[ -](\d+).*", k))),
        default=0,
    )
    audit = Audit(mixed, [1742], floor=0)
    check("form-blind reader answers 1708 on the fixture", form_blind == 1708)
    check("this reader answers 1742 on the fixture", audit.last_recorded == 1742)

    # --- 3. The reader `mnemosyne-cli --list-changelog` uses -----------------
    #
    # Lexicographic: 'R9...' sorts above 'R1...', so the "newest" entry of a
    # store whose rounds passed 1000 is whatever happened in the 900s.
    lex = ["R999", "R1742"]
    check("lexicographic newest is R999", max(lex) == "R999")
    check("round-order newest is 1742", Audit(lex, [], floor=0).last_recorded == 1742)

    # --- 4. The gap this gate exists to refuse ------------------------------
    tip = Audit(["R1742"], [1742, 1743, 1744, 1745], floor=1359)
    check("a skipped round is named", tip.missing == [1743, 1744])
    check("the round in progress is exempt", tip.in_progress == 1745)
    check("the exemption is reported", tip.in_progress_missing)
    check("a skipped round fails the check", not tip.ok())

    # The counterfactual on the exemption itself: exempting every round would
    # make the gate silent, which is the state this file replaces.
    closed = Audit(["R1742", "R1743", "R1744"], [1742, 1743, 1744, 1745], floor=1359)
    check("a closed ledger passes with a round in progress", closed.ok())
    check("and still says the tip is unwritten", closed.in_progress_missing)

    # --- 5. The floor reports; it does not hide -----------------------------
    old = Audit(["R1742"], [1300, 1742], floor=1359)
    check("a pre-floor gap is not refused", old.ok())
    check("a pre-floor gap is counted", old.below_floor == [1300])

    # --- 6. An unparseable key is a refusal, not a skip ----------------------
    novel = Audit(["R1742", "PHASE-C-KICKOFF"], [1742], floor=1359)
    check("an unknown key form fails the check", not novel.ok())
    check("and is named", novel.unknown == ["PHASE-C-KICKOFF"])
    declared = Audit(["R1742", "SCE-RFC-002-004"], [1742], floor=1359)
    check("a declared non-round key passes", declared.ok())
    check("and is not counted as a round", declared.recorded == [1742])

    # --- 7. The population is the tally's, not a second copy of it ----------
    import phase_b_tally

    check("the round parse is shared with the tally", rounds_in is phase_b_tally.rounds_in)
    check(
        "and it reads the same subjects",
        rounds_in("chore(tools): R1744 two axes are looked at again") == [1744],
    )

    # ★★★★★ R2028 — THE ORACLE, against this repository's real history.
    #
    # Every case above hands the two pure readers a fixture string, which is
    # what makes them testable at all — and R1828 split `git_subjects` out
    # precisely so both read ONE `git log`. Nothing then watched that log.
    # A `git_subjects` answering the empty string would make this tool report
    # zero rounds and zero roundless subjects, and every case above would pass;
    # `tools/oracle_census.py` counts exactly that shape.
    subjects = git_subjects()
    check("the history oracle answers this repository's subjects", subjects.count("\n") > 100)
    check(
        "and they are subjects rather than a log — one line each, no diff",
        all(not line.startswith("commit ") for line in subjects.splitlines()),
    )
    # ⚠★★★★★ TWO WRONG GUESSES, BOTH CORRECTED BY THE RUN, and the second is
    # worth more than the assertion it produced. The first draft asked for
    # ASCENDING and was answered `[2027, 2026, 2025, …]`; the second asked for
    # strictly DESCENDING and was answered *18 adjacent pairs go the other way*
    # — because commit order is not round order (a round's commit can land
    # after the next round's, and this history has eighteen such places).
    #
    # ⇒ what is true, and what a reader of this tool actually takes off the
    # front: it is a POPULATION with no repeats, and the newest round leads it.
    seen = git_rounds(subjects)
    check("the round population it feeds is not empty", len(seen) > 100)
    check("and no round is counted twice", len(seen) == len(set(seen)))
    check("and the newest round leads it, which is what this tool reports", seen[0] == max(seen))

    for name in failures:
        print(f"selftest FAIL: {name}")
    print(f"changelog_rounds selftest: {len(failures)} failure(s)")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="exit 1 on an unrecorded round")
    ap.add_argument("--selftest", action="store_true", help="run this module's tests")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    subjects = git_subjects()
    audit = Audit(store_keys(), git_rounds(subjects))
    report(audit)
    # R1828 — printed every run, over budget or not, for the reason the
    # build-cache size is: a number that only speaks when it fires leaves the
    # trend unseen, and this one replaced a prose stamp that had gone stale
    # unnoticed. It is a population, not a verdict — see `roundless_subjects`.
    roundless = roundless_subjects(subjects)
    total = len([s for s in subjects.splitlines() if s.strip()])
    print(
        f"subjects naming no round: {len(roundless)} of {total} "
        f"(a commit closing no round carries none legitimately; not a refusal)"
    )
    if args.check and not audit.ok():
        print("\nchangelog_rounds: the round ledger has a hole (above)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
