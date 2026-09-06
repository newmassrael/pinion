#!/usr/bin/env python3
"""R2054 — a walk that SPELLS a screen's painted address, held to a ratchet.

A screen in this workspace names every painted mark with a dotted address —
`<screen>.<family>.<key>`. Those addresses have declaring sites now: the screen
composes them in one place, the framework composes the ones it paints, and each
of them publishes the result so a reader is HANDED the address rather than
writing it out again.

A walk is Python and cannot call any of that. Its answer is to ask: the screen's
specification carries a role's row address, a form row's control, a rail seat,
and the prefix each part of a form row is addressed under. A walk that spells one
instead is a second, unchecked copy of the composition, and a wrong letter there
does not fail loudly — it looks for a mark that is not there, so the walk reports
that the SCREEN did not paint it.

# ★ What this counts, and why it is not "the defect count"

A spelled address is not always a defect. Some marks have no published
derivation to be handed — a walk about a card names the card. What is true is
that a spelled address CANNOT be checked against the paint, and that the number
of them per family is the size of what is left to convert. So this gate does not
demand zero. It PINS: a family's count may fall or hold, never rise, and a family
that reaches zero is pinned there — which is what stops a converted family from
quietly reacquiring a speller two rounds later.

★★★★★ That pinning is this tool's whole reason to exist. R2049-R2053 converted
five families and each round's gate was a Rust test that could only see Rust; the
93 walk sites this round paid off had no gate at all behind them, and the debt's
own instalment table was hand-maintained prose whose numbers went stale the round
after they were written. A number in prose is not a measurement.

# ★★ Counted with `ast`, on this project's standing rule that a census reads
# structure

A regex over source text cannot tell an address from a sentence about one, and
this file's own prose spells several. The needle is ANCHORED at the start of a
string literal, so a docstring that MENTIONS `lab.form.control.x` mid-sentence is
not a site while a string that IS that address is. Comments are not in the tree at
all. The selftest pins both.

# ⚠ The harness is in the population

R1783 learned this the hard way on the demo clock: the defect was not only in the
walks written against `tools/rpc_verify.py`, it was in `rpc_verify.py`. A shared
helper that spells an address spreads it to every walk that calls it, so it is
counted here beside them.

# ⚠⚠ What this gate CANNOT see, said rather than left to be discovered

The population is Python: the walks and the harness. The RUST readers — a screen's
own modules, and another crate reading a screen's marks — are not in it, and are
held instead by each screen's own address gate (`r2049_…`, `r2050_…`, `r2053_…`,
which read their sources with `include_str!`). So this tool's number is the WALKS'
remainder, not the tree's. Two counts, two gates, and neither is the whole; asking
this one "is the address debt repaid" gets an answer about a third of the tree.

Usage:
    python3 tools/painted_addresses.py --check         # the ratchet (pre-push)
    python3 tools/painted_addresses.py --selftest      # its own tests
    python3 tools/painted_addresses.py --owed          # the work order
    python3 tools/painted_addresses.py --list <stem>   # every site in a family
    python3 tools/painted_addresses.py --write-budget            # re-pin
    python3 tools/painted_addresses.py --write-budget --pin lab.form
                                       # ...and record a family as converted
"""

from __future__ import annotations

import argparse
import ast
import functools
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGET = ROOT / "docs" / "painted-address-budget.tsv"

#: A painted address, anchored at the start of a string literal.
#:
#: Three segments at least, because that is what an address IS in this tree: the
#: screen, the family, and the key. Two segments are a PREFIX — `lab.form`,
#: `shell.rail` — and a walk legitimately holds one of those, because that is the
#: shape the screen publishes and hands it.
#:
#: ⚠ Anchored on purpose. `^` is what separates a string that is an address from
#: a sentence that talks about one, and this file is full of the latter.
ADDRESS = re.compile(r"^([a-z][a-z0-9_]*\.[a-z][a-z0-9_]*)\.")


def _imported_helpers(walks: list[Path]) -> list[Path]:
    """The modules under `tools/` that the walks actually import.

    ★★★★★ DERIVED, not named. The first draft of this function listed
    `rpc_verify.py` by hand — the one shared module its author happened to be
    editing — and that is R2053's defect verbatim, committed by the round whose
    subject was R2053's defect: a gate whose population is a hand-written list
    cannot see the member nobody thought of.

    Measured when this was fixed: the walks import THREE modules from `tools/`,
    not one. The two the hand-written list omitted spell no address today, so
    the blindness had no consequence yet — which is exactly how this class stays
    invisible until it costs something. Asking the imports means a helper that
    joins the corpus tomorrow is in the population the day it does.

    Import lines only, read structurally like everything else here: a module
    named in a comment or a docstring is not an import.
    """
    found: dict[str, Path] = {}
    for walk in walks:
        try:
            tree = ast.parse(walk.read_text(encoding="utf-8"), filename=str(walk))
        except SyntaxError:
            continue
        for node in ast.walk(tree):
            names: list[str] = []
            if isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                names = [node.module.split(".")[0]]
            elif isinstance(node, ast.Import):
                names = [alias.name.split(".")[0] for alias in node.names]
            for name in names:
                candidate = ROOT / "tools" / f"{name}.py"
                if name not in found and candidate.exists():
                    found[name] = candidate
    return [found[name] for name in sorted(found)]


@functools.lru_cache(maxsize=1)
def sources() -> tuple[Path, ...]:
    """Every python file that drives a window, plus the harness they share.

    Derived from the tree rather than listed, on R2053's lesson: a gate whose
    population is a hand-written list is this debt's own defect, one level up —
    that round found a gate reading 6 of a crate's 11 modules, and the module
    nobody was reading had been spelling addresses the whole time.

    ⚠ Held for the life of the process, because deriving it PARSES the whole
    corpus and three callers want it. Measured before this: the push step cost
    8.3s, most of it re-derivation — `census` alone parsed 721 walks to find the
    helpers and then 725 files to count them. A tuple rather than a list, so a
    caller cannot edit the cached answer for everyone after it.

    ⚠⚠ The cache means a run that CHANGED the corpus mid-flight would answer
    from before the change. Nothing here does; every caller reads.
    """
    walks = sorted((ROOT / "tools" / "demos").glob("*.py"))
    return tuple(walks) + tuple(_imported_helpers(walks))


def _skipped(tree: ast.AST) -> set[int]:
    """The `id()` of every string node this census must not read on its own.

    Two kinds, for two different reasons:

    * a DOCSTRING is prose. A module explaining which address a mark is painted
      at is documentation, not a second copy of the composition.
    * an f-string's own literal HALVES, because `ast.walk` yields the
      `JoinedStr` and then yields those halves again. ★ The selftest is what
      found this: `f"lab.form.remove.{key}"` was counted twice, which would have
      pinned a budget at double the truth and let a family quietly acquire a
      second speller while the number held.
    """
    out: set[int] = set()
    for node in ast.walk(tree):
        if isinstance(
            node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
        ):
            first = node.body[0] if node.body else None
            if (
                isinstance(first, ast.Expr)
                and isinstance(first.value, ast.Constant)
                and isinstance(first.value.value, str)
            ):
                out.add(id(first.value))
        elif isinstance(node, ast.JoinedStr):
            for part in node.values:
                if isinstance(part, ast.Constant):
                    out.add(id(part))
    return out


def sites(path: Path) -> list[tuple[int, str]]:
    """`(line, family stem)` for every spelled painted address in `path`."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    except SyntaxError:
        # A file this tool cannot parse is a file it must not judge.
        return []
    skip = _skipped(tree)
    found: list[tuple[int, str]] = []
    for node in ast.walk(tree):
        pieces: list[str] = []
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            if id(node) in skip:
                continue
            pieces = [node.value]
        elif isinstance(node, ast.JoinedStr):
            # An f-string's literal halves. `f"{parts['add']}{key}"` has none,
            # which is the shape a converted site takes.
            pieces = [
                part.value
                for part in node.values
                if isinstance(part, ast.Constant) and isinstance(part.value, str)
            ]
        for piece in pieces:
            match = ADDRESS.match(piece)
            if match:
                found.append((node.lineno, match.group(1)))
    return sorted(found)


def scan() -> dict[str, list[tuple[str, int]]]:
    """Every spelled site in the corpus, indexed by family stem.

    ⚠ ONE pass. The first draft answered `--owed` by asking `where()` per
    family, which re-parsed all 250-odd walks 109 times over and did not finish
    inside two minutes — a census whose cost grows with the size of the answer
    is one nobody runs, and a gate nobody runs is prose.
    """
    index: dict[str, list[tuple[str, int]]] = {}
    for path in sources():
        name = str(path.relative_to(ROOT))
        for line, stem in sites(path):
            index.setdefault(stem, []).append((name, line))
    return index


def census(index: dict[str, list[tuple[str, int]]] | None = None) -> dict[str, int]:
    """How many sites spell an address under each family stem."""
    index = scan() if index is None else index
    return {stem: len(found) for stem, found in index.items()}


def read_budget() -> dict[str, int]:
    if not BUDGET.exists():
        return {}
    out: dict[str, int] = {}
    for line in BUDGET.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        stem, _, count = line.partition("\t")
        out[stem] = int(count)
    return out


def write_budget(counts: dict[str, int], pin: str | None = None) -> None:
    """Pin what stands, and keep a converted family pinned at zero.

    ★ A family the scan no longer finds is carried forward at 0 rather than
    dropped. A dropped row would let the next round re-acquire a speller in a
    family a previous one paid off, and the gate would call that a NEW family
    only if the stem were new — which it is not.

    ★★ `pin` is how a family FIRST reaches zero. A converted family leaves no
    trace in the corpus to be found by scanning — that is what converting it
    means — so the round that pays one off says so, and the claim is checked at
    the moment it is made rather than taken on trust.
    """
    kept = dict(counts)
    for stem in read_budget():
        kept.setdefault(stem, 0)
    if pin is not None:
        standing = counts.get(pin, 0)
        if standing:
            raise SystemExit(
                f"painted-addresses: refusing to pin {pin} at zero — the corpus "
                f"still spells it {standing} time(s). Ask "
                f"`--list {pin}` for where."
            )
        kept[pin] = 0
    lines = [
        "# R2054 — how many sites in the walk corpus SPELL a painted address",
        "# instead of asking the screen for it, by family stem.",
        "#",
        "# The gate allows a count to FALL or hold, refuses a rise, and refuses a",
        "# family stem it has never seen. A family at 0 is PINNED there: it was",
        "# converted, and a speller reappearing in it is a regression.",
        "#",
        "# The fix at a site is to read the address off the wire — a screen",
        "# publishes its roles' rows, its form rows' controls, the prefix each",
        "# part of a form row is addressed under, and its rail's seats.",
        "#",
        "# Rewritten by `tools/painted_addresses.py --write-budget`; do not",
        "# hand-edit.",
    ]
    lines += [f"{stem}\t{n}" for stem, n in sorted(kept.items())]
    BUDGET.write_text("\n".join(lines) + "\n", encoding="utf-8")


def check() -> int:
    index = scan()
    now, before = census(index), read_budget()
    if not before:
        print(
            "painted-addresses: no budget yet — run --write-budget once to pin "
            "what stands",
            file=sys.stderr,
        )
        return 1
    risen = [(s, before.get(s, 0), n) for s, n in now.items() if n > before.get(s, 0)]
    if risen:
        print(
            "painted-addresses: a walk spells a painted address it could ask for",
            file=sys.stderr,
        )
        for stem, was, is_now in sorted(risen):
            known = "" if stem in before else " (a family this gate has not seen)"
            print(f"  {stem}: {was} -> {is_now}{known}", file=sys.stderr)
            for path, line in index.get(stem, []):
                print(f"      {path}:{line}", file=sys.stderr)
        print(
            "painted-addresses: a spelled address is a second copy of a\n"
            "            composition the screen already publishes, and a wrong\n"
            "            letter in it reads as the screen not painting the mark.\n"
            "            Ask the screen — see `rpc_verify.form_part_prefixes`\n"
            "            and `rpc_verify.address_prefix`. If the family really\n"
            "            has nothing published to derive from, say so and re-run\n"
            "            `python3 tools/painted_addresses.py --write-budget`.",
            file=sys.stderr,
        )
        return 1
    total_now, total_before = sum(now.values()), sum(before.values())
    pinned = sum(1 for stem, n in before.items() if n == 0)
    gone = total_before - total_now
    trend = f", {gone} fewer than the budget" if gone > 0 else ""
    print(
        f"painted-addresses: {total_now} spelled site(s) in {len(now)} family/ies"
        f"{trend}; {pinned} family/ies pinned at zero"
    )
    return 0


def owed() -> int:
    """The work order: the families with the most sites left to convert."""
    index = scan()
    if not index:
        print("painted-addresses: nothing spelled anywhere")
        return 0
    print(f"{'sites':>6}  {'files':>5}  family")
    for stem, found in sorted(index.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        files = len({path for path, _ in found})
        print(f"{len(found):>6}  {files:>5}  {stem}")
    total = sum(len(f) for f in index.values())
    print(f"{total:>6}  total across {len(index)} family/ies")
    return 0


def _counts(source: str) -> list[str]:
    import tempfile

    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as fh:
        fh.write(source)
        tmp = Path(fh.name)
    try:
        return [stem for _, stem in sites(tmp)]
    finally:
        tmp.unlink()


def selftest() -> int:
    cases: list[tuple[str, str, list[str]]] = [
        ("a whole address is a site", 'press(tf, "lab.form.remove.mode")\n', ["lab.form"]),
        (
            "an f-string that spells the prefix is a site",
            'press(tf, f"lab.form.remove.{key}")\n',
            ["lab.form"],
        ),
        (
            "★ an f-string handed the prefix is NOT — this is the converted shape",
            'press(tf, f"{parts[\'remove\']}{key}")\n',
            [],
        ),
        (
            "a two-segment prefix is what the screen publishes, not a spelling",
            'assert tag.startswith("shell.rail")\n',
            [],
        ),
        (
            "★★ a docstring ABOUT an address is prose, not a site",
            '"""the row is painted at lab.form.remove.mode."""\n',
            [],
        ),
        (
            "★★ and a docstring that STARTS with one is prose too",
            '"""lab.form.remove.mode is where the seat goes."""\n',
            [],
        ),
        ("a comment is not in the tree at all", '# lab.form.remove.mode\n', []),
        (
            "★★★ the needle is anchored: a sentence carrying one is not a site",
            'ok("the seat at lab.form.remove.mode is gone", True)\n',
            [],
        ),
        (
            "two families in one file are two sites",
            'a = "lab.form.remove.mode"\nb = "shell.rail.lab"\n',
            ["lab.form", "shell.rail"],
        ),
        (
            "an rpc method path is not an address",
            'call(tf, "scene/containment")\n',
            [],
        ),
        (
            "a transport address a walk is ABOUT is not a painted one",
            'OUTSIDE = "tcp/10.0.0.21:7449"\n',
            [],
        ),
        (
            "a file name is not an address",
            'p = "analyzer-inspector-spec.json"\n',
            [],
        ),
        (
            "an uppercase name is not this tree's address shape",
            'x = "Lab.Form.Remove"\n',
            [],
        ),
        (
            "★ a family stem is the first two segments, whatever the key holds",
            'a = "lab.form.item.listen.endpoints.0"\n',
            ["lab.form"],
        ),
    ]
    failed = 0
    for name, source, want in cases:
        got = sorted(_counts(source))
        if got != sorted(want):
            failed += 1
            print(f"FAIL: {name}: want {want}, got {got}", file=sys.stderr)

    # ★★★★★ THE POPULATION IS ASSERTED, because a gate that cannot see a file
    # answers zero about it and reads as a pass. The expected roster is derived
    # here by a DIFFERENT method from the one under test — a line-wise read of
    # the import statements rather than a parse — so the two sides do not share
    # a derivation and cannot agree by construction.
    walks = sorted((ROOT / "tools" / "demos").glob("*.py"))
    wanted: set[str] = set()
    for walk in walks:
        for line in walk.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            for prefix in ("from ", "import "):
                if not line.startswith(prefix):
                    continue
                name = line[len(prefix) :].split()[0].split(".")[0].split(",")[0]
                if (ROOT / "tools" / f"{name}.py").exists():
                    wanted.add(name)
    have = {p.stem for p in sources()} - {p.stem for p in walks}
    missing = sorted(wanted - have)
    if missing:
        failed += 1
        print(
            f"FAIL: the walks import shared module(s) this census cannot see: "
            f"{missing}",
            file=sys.stderr,
        )
    if not wanted:
        failed += 1
        print(
            "FAIL: no shared module was found at all, so the assertion above "
            "cannot fail and is not checking anything",
            file=sys.stderr,
        )

    # ★★★★★ And the properties the BUDGET carries, driven against the tree it
    # describes rather than described in prose beside it. One scan, reused.
    now = census()
    budget = read_budget()

    # A family the budget pins at zero must really be spelled nowhere. This is
    # what makes a conversion durable: R2049-R2053 converted five families with
    # nothing behind them but a Rust test that could not see a walk.
    broken = [(s, now[s]) for s, n in budget.items() if n == 0 and now.get(s, 0) > 0]
    if broken:
        failed += 1
        print(
            f"FAIL: family/ies pinned at zero are spelled again: {broken}",
            file=sys.stderr,
        )

    # A budget describing a tree that has moved on is a budget nobody can read.
    if budget:
        stale = [s for s in now if s not in budget]
        if stale:
            failed += 1
            print(
                f"FAIL: the census finds family/ies the budget has never seen: "
                f"{sorted(stale)}",
                file=sys.stderr,
            )
        gone = [s for s, n in budget.items() if n > 0 and s not in now]
        if gone:
            failed += 1
            print(
                f"FAIL: the budget charges family/ies the census cannot find — "
                f"they were converted and never re-pinned: {sorted(gone)}. Run "
                f"--write-budget.",
                file=sys.stderr,
            )

    total = len(cases) + 5
    print(f"painted_addresses selftest: {total - failed} of {total} cases OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="the ratchet")
    parser.add_argument("--write-budget", action="store_true")
    parser.add_argument(
        "--pin",
        metavar="STEM",
        help="with --write-budget: record a family as converted, at zero. "
        "Refused unless the corpus really spells it nowhere.",
    )
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--owed", action="store_true", help="the work order")
    parser.add_argument("--list", metavar="STEM", help="every site in one family")
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.write_budget:
        write_budget(census(), pin=args.pin)
        pinned = f", {args.pin} pinned at zero" if args.pin else ""
        print(
            f"painted-addresses: budget written to {BUDGET.relative_to(ROOT)}{pinned}"
        )
        return 0
    if args.pin:
        print("painted-addresses: --pin is for --write-budget", file=sys.stderr)
        return 2
    if args.owed:
        return owed()
    if args.list:
        found = scan().get(args.list, [])
        for path, line in found:
            print(f"{path}:{line}")
        print(f"{len(found)} site(s) under {args.list}")
        return 0
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
