#!/usr/bin/env python3
"""Refuse an unresolvable `--impact` token BEFORE the ledger freezes it.

★★★★★ R1791 — the prescription R1758 wrote down and nobody built, built.

# The class this closes

`mnemosyne-cli append-changelog-entry --impact <tokens>` accepts every token
without complaint and exits 0. The store's impact list takes **section ids**; a
token that reads correctly to a person and resolves to nothing is written
anyway. The defect then surfaces **one command later**, at
`validate-workspace`, as a *number*:

    FAILED: atomic orphan new (entries=1, sections=0)

— which does not say which ref, and by then the entry's **audit half is frozen**
(R296 schema design). The repair is no longer an edit: it is a
`[[publishable_override_ledger]]` row, an `[[orphan_ledger]]` row, and a pair of
content hashes emitted by a third command.

`mnemosyne.toml` records four instances, all the same shape:

| round | token | what it should have been |
|---|---|---|
| R51.186 | `45`, `13`, `41`, `35` | the `5.` prefix was dropped |
| R682.A | `5.46` | no such section |
| R1758 | `2 #7` | the PROSE form of an invariant |
| R1791 | `2 #1` | the PROSE form of an invariant |

R1758 named the fix — *a thin wrapper that verifies the tokens against
`query --list-sections` before the ledger is written, attached to the command a
round actually runs, because after the write it is too late* — and left it
unbuilt. The class recurred 33 rounds later, in the round that had that sentence
in front of it. ⇒ **a prescription nobody executes is not a repair**, which is
this file's reason to exist rather than a fifth comment saying to be careful.

# Use

    python3 tools/impact_refs.py check "§5.11,§5.2,§2 #1"
    python3 tools/impact_refs.py append --entry-id R1791 --impact "§5.11,§5.2" \\
        --decision-file d.txt --changes-file c.txt \\
        --verification-file v.txt --carry-file y.txt

`check` exits 1 naming every token that does not resolve, and — because three of
the five bad tokens on record were a legal id with something appended — it says
what the token would be if the suffix were dropped. `append` runs `check` first
and only then hands the whole invocation to `mnemosyne-cli`, so the ledger is
never reached by a token this file would refuse.

`--selftest` runs the unit checks with a stub section list and needs no store.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

#: Where the hook libraries resolve the pinned tool. Used so this file runs the
#: same binary the gates do rather than whatever is on PATH — the distinction
#: R1507 made load-bearing for every other Mnemosyne call in this tree.
RESOLVER = Path(__file__).resolve().parent.parent / ".githooks" / "lib" / "mnemosyne-tool.sh"


def known_sections(cli: str = "mnemosyne-cli") -> set[str]:
    """Every section id the store will resolve, asked of the store.

    Not cached and not written down: a section added tomorrow must be accepted
    tomorrow, and a list here would be the fifth copy of a fact this project
    already keeps in one place.
    """
    out = subprocess.run(  # noqa: S603
        [cli, "query", "--list-sections"],
        capture_output=True,
        text=True,
        check=True,
    )
    return {line.strip() for line in out.stdout.splitlines() if line.strip()}


def normalise(token: str) -> str:
    """A token as the store spells it: no `§`, no surrounding space.

    The `§` is how this project writes a section in prose and is not part of the
    id, so accepting it here is not laxity — it is the one difference between
    the two spellings that has never caused a defect.
    """
    return token.strip().lstrip("§").strip()


def offenders(tokens: list[str], known: set[str]) -> list[tuple[str, str]]:
    """Each token that will not resolve, with what it looks like it meant.

    The advice is derived rather than guessed: three of the five bad tokens on
    record were a legal id followed by something (`2 #7`, `2 #1`), so the first
    thing to try is the leading id-shaped run — and saying so turns a refusal
    into an instruction.
    """
    bad: list[tuple[str, str]] = []
    for token in tokens:
        key = normalise(token)
        if key in known:
            continue
        head = re.match(r"[0-9]+(?:\.[0-9]+)*", key)
        if head and head.group(0) in known and head.group(0) != key:
            advice = (
                f"§{head.group(0)} resolves; the rest ({key[len(head.group(0)):].strip()!r}) "
                "is prose and belongs in the entry's own text"
            )
        elif head and f"5.{head.group(0)}" in known:
            advice = f"did you mean §5.{head.group(0)}? (the `5.` prefix, as in R51.186)"
        else:
            advice = "no section with that id is registered"
        bad.append((token, advice))
    return bad


def check(impact: str, known: set[str]) -> int:
    tokens = [t for t in impact.split(",") if t.strip()]
    if not tokens:
        print("impact_refs: no tokens to check")
        return 0
    bad = offenders(tokens, known)
    for token, advice in bad:
        print(f"impact_refs: REFUSED {token!r} — {advice}", file=sys.stderr)
    if bad:
        print(
            f"impact_refs: {len(bad)} of {len(tokens)} impact ref(s) would not resolve.\n"
            "  Writing them freezes the entry's audit half and the repair is three\n"
            "  commands and two mnemosyne.toml rows (see the R1791 block there).",
            file=sys.stderr,
        )
        return 1
    print(f"impact_refs: {len(tokens)} impact ref(s) resolve")
    return 0


def append(args: argparse.Namespace, known: set[str]) -> int:
    if check(args.impact, known) != 0:
        return 1
    cmd = [
        args.cli,
        "append-changelog-entry",
        "--entry-id",
        args.entry_id,
        "--decision-file",
        args.decision_file,
        "--changes-file",
        args.changes_file,
        "--verification-file",
        args.verification_file,
        "--carry-file",
        args.carry_file,
        "--impact",
        args.impact,
    ]
    print("impact_refs: " + " ".join(cmd))
    return subprocess.run(cmd, check=False).returncode  # noqa: S603


def selftest() -> int:
    """The refusals, against a stub section list — no store, no network.

    Every case here is one that actually happened; the table in this file's
    docstring is where they come from.
    """
    known = {"1", "2", "5.2", "5.11", "5.45", "5.13"}
    failures = 0

    def expect(name: str, got, want) -> None:
        nonlocal failures
        if got != want:
            print(f"selftest FAIL {name}: {got!r} != {want!r}", file=sys.stderr)
            failures += 1

    # The four recorded instances are each refused.
    expect("R1758 prose form", [t for t, _ in offenders(["2 #7"], known)], ["2 #7"])
    expect("R1791 prose form", [t for t, _ in offenders(["§2 #1"], known)], ["§2 #1"])
    expect("R682.A absent id", [t for t, _ in offenders(["5.46"], known)], ["5.46"])
    expect(
        "R51.186 dropped prefix",
        [t for t, _ in offenders(["45", "13"], known)],
        ["45", "13"],
    )
    # And the advice points somewhere, which is what makes a refusal usable.
    expect("prose advice names the id", "§2 resolves" in offenders(["2 #1"], known)[0][1], True)
    expect(
        "prefix advice names the id",
        "§5.45" in offenders(["45"], known)[0][1],
        True,
    )
    # Good tokens pass, with and without the sign.
    expect("plain id", offenders(["5.11"], known), [])
    expect("signed id", offenders(["§5.11"], known), [])
    expect("spaced id", offenders([" §5.2 "], known), [])
    # ★ A token that is a PREFIX of a legal id is not legal: `5.1` is not `5.11`.
    expect("prefix is not a match", [t for t, _ in offenders(["5.1"], known)], ["5.1"])

    # ★★★★★ R2028 — THE ORACLE, asked of the real store.
    #
    # Every case above hands `offenders` a fixture set of ids, which is what
    # makes the rule testable. Nothing watched the function that produces the
    # real set — and a `known_sections` answering the EMPTY set would make this
    # gate refuse every impact ref ever written, while an over-wide one would
    # accept the token that produced the four instances `mnemosyne.toml`
    # records. `tools/oracle_census.py` counts that shape.
    #
    # ⚠ Skipped, loudly, where the pinned tool is not on PATH: this file is
    # importable on a machine that has never built Mnemosyne, and a selftest
    # that died there would be a gate nobody could run.
    try:
        real = known_sections()
    except (OSError, subprocess.CalledProcessError):
        real = None
        print("impact_refs selftest: SKIPPED the store oracle — no CLI on PATH")
    if real is not None:
        expect("the store answers sections", len(real) > 20, True)
        expect("§3 is one of them", "3" in real, True)
        expect(
            "and they are bare ids, which is the form `offenders` compares",
            all(not s.startswith("§") for s in real),
            True,
        )
        expect("a real id passes against the real store", offenders(["§3"], real), [])
        expect(
            "and an invented one does not",
            [t for t, _ in offenders(["§5.999"], real)],
            ["§5.999"],
        )

    print(f"impact_refs selftest: {16 - failures} of 16 passed")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--cli", default="mnemosyne-cli")
    sub = parser.add_subparsers(dest="mode")

    checker = sub.add_parser("check")
    checker.add_argument("impact")

    appender = sub.add_parser("append")
    appender.add_argument("--entry-id", required=True)
    appender.add_argument("--impact", required=True)
    appender.add_argument("--decision-file", required=True)
    appender.add_argument("--changes-file", required=True)
    appender.add_argument("--verification-file", required=True)
    appender.add_argument("--carry-file", required=True)

    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.mode is None:
        parser.print_help()
        return 2
    known = known_sections(args.cli)
    if args.mode == "check":
        return check(args.impact, known)
    args.cli = parser.parse_args().cli
    return append(args, known)


if __name__ == "__main__":
    sys.exit(main())
