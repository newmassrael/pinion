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
  `type(scope): R<N> …` is invisible to BOTH gates rather than to one. Measured
  at R1746: no such subject exists in this history, and none carries a `!`
  breaking marker either — which is the only reason the shared parse is safe to
  rely on and is worth re-measuring if that convention ever loosens.
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


def git_rounds() -> list[int]:
    """Rounds git knows about. Git is the authority on which rounds EXIST — if
    the ledger were also the census, a round that never appended would be
    invisible instead of reported."""
    out = subprocess.run(
        ["git", "log", "--format=%s", "--no-merges"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return rounds_in(out)


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

    audit = Audit(store_keys(), git_rounds())
    report(audit)
    if args.check and not audit.ok():
        print("\nchangelog_rounds: the round ledger has a hole (above)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
