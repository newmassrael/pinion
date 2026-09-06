#!/usr/bin/env python3
"""R2056 — the entry-point document, held to the rounds git actually has.

`CLAUDE.md` names `docs/SEED_PROMPT.md` as the first thing to read on entering
this repository, and the repayment loop's own instructions repeat it. Its "next
slice" section is one of three places a round is chosen from. So a stale one
does not merely go unread — it points the next round at work that was current
several dozen rounds ago.

# ★★★★★ Why nothing caught this, and why the usual method cannot

The file is **`.gitignore`d**, and deliberately: it is a local working file, and
a fresh clone starts its continuity from `memory/MEMORY.md`, `git log` and the
changelog instead. The price of that choice is this gate's difficulty — an
untracked file NEVER appears in the changed-path list that `pre-commit` and
`pre-push` use to decide which checks to run, so the mechanism this repository
uses for every other rule is unavailable in principle.

⇒ this check is UNCONDITIONAL. It runs on every push, reads the file if it is
there, and says what it found either way.

# ★★ And it fails OPEN when the file is absent

A fresh clone has no SEED, and that is correct rather than broken. Refusing
there would make the gate a reason not to clone. It prints, always — the failure
mode this exists to prevent is a check that quietly stopped happening, and one
that is silent when it passes cannot be told from one that is gone.

# ⚠ What the debt this closes had already measured

R1978.1 registered it after finding the land block at R1928 while git was at
R1977: **49 rounds**. That round then repaired the staleness BY HAND and wrote
the sentence this file exists to answer — *"performing it once is not a gate"*.
Measured again at R2055, 32 rounds after that repair, the gap was **32**. The
prose commanded, nobody executed, and the class recurred exactly as predicted.

# The rule

Every round git knows about must have a land block, EXCEPT the newest — the
round in flight legitimately pushes before it closes, which is the same
exemption `changelog_rounds.py` gives the ledger. A SEED that is AHEAD of git is
not a fault: writing the land block before the commit is the recommended order.

Usage:
    python3 tools/seed_freshness.py --check     # the gate (pre-push)
    python3 tools/seed_freshness.py --selftest  # its own tests
    python3 tools/seed_freshness.py             # report, never refuses
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEED = ROOT / "docs" / "SEED_PROMPT.md"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase_b_tally import rounds_in  # noqa: E402  (path set above)

#: A land block's heading. The file writes them as `## R2055 land = ...` while
#: it is the newest and `## R2054 land (지움 …)` or `## ~~R2022 land~~ (…)` once
#: folded — all three are a block, because what this counts is whether the round
#: is RECORDED, not how much of it is still spelled out.
LAND = re.compile(r"^##\s*~?~?\s*R(\d+)(?:\.\d+)?\s+land", re.MULTILINE)

#: How far behind git the newest land block may be.
#:
#: ONE, and the one is the round in flight. A round legitimately pushes before it
#: closes — `changelog_rounds.py` grants the ledger the same exemption in as many
#: words — so demanding zero would refuse the first push of every round.
BUDGET = 1


def land_rounds(text: str) -> list[int]:
    """Every round the entry document carries a land block for. Pure."""
    return sorted({int(m) for m in LAND.findall(text)}, reverse=True)


def git_subjects() -> str:
    return subprocess.run(
        ["git", "log", "--format=%s", "--no-merges"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    ).stdout


def verdict(text: str | None, subjects: str) -> tuple[bool, str]:
    """`(ok, sentence)` — pure in both arguments, so the selftest can drive it.

    ★ Both facts are ARGUMENTS rather than read in here. This project has paid
    for the other shape: a judgement that reaches for the world cannot be put a
    case to, so its arms go unexercised and the one that matters is the one
    nobody ran.
    """
    rounds = rounds_in(subjects)
    if not rounds:
        return True, "seed freshness: git names no round yet — nothing to be behind"
    newest = max(rounds)
    if text is None:
        return True, (
            f"seed freshness: no entry document here (a fresh clone has none); "
            f"git is at R{newest}"
        )
    lands = land_rounds(text)
    if not lands:
        return False, (
            f"seed freshness: the entry document carries NO land block at all, "
            f"and git is at R{newest}. Its own rule is to carry the last round."
        )
    have = max(lands)
    behind = newest - have
    if behind > BUDGET:
        return False, (
            f"seed freshness: the entry document's newest land block is R{have} "
            f"and git is at R{newest} — {behind} round(s) behind.\n"
            f"            It is the first thing a session reads and one of the "
            f"places the next round is chosen from, so a stale one aims the next "
            f"round at finished work.\n"
            f"            Write this round's land block into "
            f"docs/SEED_PROMPT.md and fold the one below it — that file's own "
            f"rule is to carry the last round and DELETE the rest."
        )
    ahead = " (ahead of git, which is the recommended order)" if behind < 0 else ""
    return True, (
        f"seed freshness: entry document at R{have}, git at R{newest}, "
        f"{behind} behind{ahead}; {len(lands)} land block(s)"
    )


def check() -> int:
    text = SEED.read_text(encoding="utf-8") if SEED.exists() else None
    ok, said = verdict(text, git_subjects())
    print(said, file=sys.stderr if not ok else sys.stdout)
    return 0 if ok else 1


def selftest() -> int:
    #: Each case is (name, seed text or None, git subjects, expected ok).
    HEAD = "feat(x): R2055 a thing\nfeat(x): R2054 another\n"
    cases: list[tuple[str, str | None, str, bool]] = [
        ("current is fine", "## R2055 land = x\n", HEAD, True),
        (
            "one behind is the round in flight",
            "## R2054 land = x\n",
            HEAD,
            True,
        ),
        ("two behind is stale", "## R2053 land = x\n", HEAD, False),
        (
            "★ far behind is what this exists for",
            "## R2022 land = x\n",
            HEAD,
            False,
        ),
        (
            "ahead is not a fault — the block is written before the commit",
            "## R2056 land = x\n",
            HEAD,
            True,
        ),
        (
            "★★ a missing file FAILS OPEN, because a fresh clone has none",
            None,
            HEAD,
            True,
        ),
        (
            "a file with no land block at all is refused",
            "# some other document\n",
            HEAD,
            False,
        ),
        (
            "★ a folded block still counts — what matters is that the round is "
            "recorded, not how much of it is left",
            "## R2055 land (지움 — 아래 한 줄만 남긴다)\n",
            HEAD,
            True,
        ),
        (
            "★ and the struck-through spelling too",
            "## ~~R2055 land~~ (지움)\n",
            HEAD,
            True,
        ),
        (
            "a dotted round is the same round",
            "## R2055.1 land = x\n",
            HEAD,
            True,
        ),
        (
            "the newest block wins, not the first one in the file",
            "## R2010 land = x\n## R2055 land = y\n",
            HEAD,
            True,
        ),
        (
            "★★ git naming no round cannot refuse — there is nothing to be "
            "behind, and a gate that fires on an empty history is a gate that "
            "fires in a fresh clone",
            "## R1 land = x\n",
            "chore: no round here\n",
            True,
        ),
        (
            "a heading that merely mentions a round is not a land block",
            "## R2055 was a good round\n",
            HEAD,
            False,
        ),
    ]
    failed = 0
    for name, text, subjects, want in cases:
        got, said = verdict(text, subjects)
        if got != want:
            failed += 1
            print(f"FAIL: {name}: want ok={want}, got ok={got} ({said})", file=sys.stderr)

    # ★★★★★ THE ORACLE IS EXERCISED, not only the rule it feeds.
    #
    # Every case above hands `verdict` a fixture, which is what makes those arms
    # drivable — and leaves `git_subjects`, the thing that supplies that argument
    # in production, untested. This project's oracle census refused this file for
    # exactly that and it was right: the failure it hides is a SILENT FAIL-OPEN.
    # If `git log` ever answered nothing, `git_subjects` would return "", the
    # rule would say "git names no round yet — nothing to be behind", and the
    # gate would pass forever while guarding nothing.
    subjects = git_subjects()
    if not subjects.strip():
        failed += 1
        print(
            "FAIL: `git_subjects` answered nothing — the rule would then find no "
            "round and pass vacuously, which is this gate guarding nothing",
            file=sys.stderr,
        )
    elif not rounds_in(subjects):
        failed += 1
        print(
            "FAIL: `git_subjects` answered, but no subject in it names a round "
            "this parse understands — the same vacuous pass by another route",
            file=sys.stderr,
        )

    # ★★★★★ And the gate is driven against the tree it actually guards, because
    # every case above is a fixture and a fixture cannot notice that the real
    # file stopped matching the pattern this reads it with.
    if SEED.exists():
        lands = land_rounds(SEED.read_text(encoding="utf-8"))
        if not lands:
            failed += 1
            print(
                "FAIL: the real entry document parses to zero land blocks — the "
                "pattern and the file have drifted apart",
                file=sys.stderr,
            )
    total = len(cases) + 2
    print(f"seed_freshness selftest: {total - failed} of {total} cases OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="the gate")
    parser.add_argument("--selftest", action="store_true", help="its own tests")
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.check:
        return check()
    text = SEED.read_text(encoding="utf-8") if SEED.exists() else None
    print(verdict(text, git_subjects())[1])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
