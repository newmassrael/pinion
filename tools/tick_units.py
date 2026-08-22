#!/usr/bin/env python3
"""R1783 — the demo clock's UNIT, held to a ratchet.

`RpcSubprocess.tick(dt)` advances the animation clock by `dt` **seconds**.
172 call sites across 28 demos were written `tick(16)`, meaning one frame at
60 fps. `tick(16)` advances **sixteen seconds**.

For most of those sites it never showed. Overshooting a 200 ms fade lands on
the same settled value, and `tick()`'s own docstring blesses exactly that. It
shows the moment something has a FINITE LIFETIME that is supposed to still be
running:

  * measured on `hello-analyzer-shell`, whose toast lives 2.6 s, one press
    then `tick(2.5)` leaves the sentence standing and `tick(2.7)` empties it;
  * so one `tick(16)` destroyed the thing eight demos were about, and R1778
    is what made it visible — before it, the toast left the SCREEN on expiry
    while the WIRE went on reporting the sentence, so the demos passed by
    reading something the screen had stopped showing.

This gate does not demand the backlog be zero. It PINS it: a count may fall
or hold, never rise, and a file with no entry may not acquire one. The fix at
any site is `tick_ms(16)`, which says the unit in the name.

★ Counted with `ast`, not a regex, on this project's standing rule that a
census reads structure. A regex over source text cannot tell a call from a
comment or a docstring that spells one, and this file's own prose spells
several; the selftest pins both cases rather than trusting the argument.

★★ WHAT THE FIRST RUN FOUND, which is why the harness is in the population
and not only the demos: `tools/rpc_verify.py` itself holds two of these, in
SHARED helpers that press a tag and read what changed. The defect was not
only in demos written against the harness — it was in the harness.

Usage:
    python3 tools/tick_units.py --check         # the ratchet (pre-push)
    python3 tools/tick_units.py --selftest      # its own tests
    python3 tools/tick_units.py --write-budget  # after a deliberate change
    python3 tools/tick_units.py --list          # every site, with its value
"""

from __future__ import annotations

import argparse
import ast
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGET = ROOT / "docs" / "tick-seconds-budget.tsv"

#: A `tick()` argument at or above this many seconds is being read as a
#: millisecond count by whoever wrote it. Chosen above every animation this
#: tree deliberately fast-forwards (the largest legitimate one is a 2.6 s
#: lifetime) and below the smallest accidental one (16).
SUSPECT_SECONDS = 5.0


def _sources() -> list[Path]:
    """Every python file that can drive a window, in a stable order."""
    files = sorted(p for p in (ROOT / "tools" / "demos").glob("*.py"))
    files += sorted(p for p in (ROOT / "tools").glob("*.py"))
    return files


def _literal_seconds(node: ast.AST) -> float | None:
    """The seconds a `tick(...)` call passes, when it passes a literal."""
    if not isinstance(node, ast.Call):
        return None
    func = node.func
    if not isinstance(func, ast.Attribute) or func.attr != "tick":
        return None
    args = list(node.args)
    for kw in node.keywords:
        if kw.arg == "dt":
            args.append(kw.value)
    if len(args) != 1:
        return None
    arg = args[0]
    if isinstance(arg, ast.Constant) and isinstance(arg.value, (int, float)):
        return float(arg.value)
    # A negated literal is not a thing here, and anything computed is the
    # author having thought about it.
    return None


def sites(path: Path) -> list[tuple[int, float]]:
    """`(line, seconds)` for every suspect `tick()` call in `path`."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    except SyntaxError:
        # A file this tool cannot parse is a file it must not judge.
        return []
    found = []
    for node in ast.walk(tree):
        seconds = _literal_seconds(node)
        if seconds is not None and seconds >= SUSPECT_SECONDS:
            found.append((node.lineno, seconds))
    return sorted(found)


def census() -> dict[str, int]:
    counts: dict[str, int] = {}
    for path in _sources():
        found = sites(path)
        if found:
            counts[str(path.relative_to(ROOT))] = len(found)
    return counts


def read_budget() -> dict[str, int]:
    if not BUDGET.exists():
        return {}
    out: dict[str, int] = {}
    for line in BUDGET.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        path, _, count = line.partition("\t")
        out[path] = int(count)
    return out


def write_budget(counts: dict[str, int]) -> None:
    lines = [
        "# R1783 — the demo clock's unit budget. One row per file that still",
        "# passes a `tick()` a number of SECONDS large enough to be somebody's",
        "# millisecond count, with how many such sites it holds.",
        "#",
        "# The gate allows a count to FALL or hold and refuses a rise, and it",
        "# refuses a file that acquires its first one. The fix is `tick_ms(16)`.",
        "#",
        "# Rewritten by `tools/tick_units.py --write-budget`; do not hand-edit.",
    ]
    lines += [f"{path}\t{n}" for path, n in sorted(counts.items())]
    BUDGET.write_text("\n".join(lines) + "\n", encoding="utf-8")


def check() -> int:
    now, before = census(), read_budget()
    risen = [(p, before.get(p, 0), n) for p, n in now.items() if n > before.get(p, 0)]
    total_now, total_before = sum(now.values()), sum(before.values())

    if risen:
        print("tick-units: a demo gained a SECONDS-sized tick", file=sys.stderr)
        for path, was, is_now in sorted(risen):
            print(f"  {path}: {was} -> {is_now}", file=sys.stderr)
            for line, seconds in sites(ROOT / path):
                print(f"      line {line}: tick({seconds:g}) = {seconds:g} SECONDS", file=sys.stderr)
        print(
            "tick-units: `tick()` takes SECONDS. If the site means one frame,\n"
            "            write `tick_ms(16)`. If it really means a fast-forward\n"
            "            of that many seconds, say so and re-run\n"
            "            `python3 tools/tick_units.py --write-budget`.",
            file=sys.stderr,
        )
        return 1

    gone = total_before - total_now
    trend = f", {gone} fewer than the budget" if gone > 0 else ""
    print(f"tick-units: {total_now} site(s) in {len(now)} file(s){trend}")
    return 0


def _selftest_counts(source: str) -> int:
    import tempfile

    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as fh:
        fh.write(source)
        tmp = Path(fh.name)
    try:
        return len(sites(tmp))
    finally:
        tmp.unlink()


def selftest() -> int:
    cases: list[tuple[str, str, int]] = [
        ("a bare seconds-sized tick is counted", "app.tick(16)\n", 1),
        ("a frame-sized tick is not", "app.tick(0.016)\n", 0),
        ("a deliberate short fast-forward is not", "app.tick(0.5)\n", 0),
        ("the boundary itself is counted", f"app.tick({SUSPECT_SECONDS})\n", 1),
        ("just under the boundary is not", "app.tick(4.9)\n", 0),
        ("tick_ms is a different method", "app.tick_ms(16)\n", 0),
        ("the keyword spelling is read", "app.tick(dt=16)\n", 1),
        ("a computed argument is the author having thought", "app.tick(n)\n", 0),
        ("two receivers, two sites", "app.tick(16)\nlab.tick(16)\n", 2),
        # ★★★★★ THE CASE A REGEX GETS WRONG, and it is not hypothetical: the
        # first draft of this census counted `rpc_verify.py`'s own docstring,
        # which explains the trap rather than containing it.
        ('a docstring ABOUT tick(16) is prose', '"""see app.tick(16) below."""\n', 0),
        ("a comment about it is prose too", "# app.tick(16) is sixteen seconds\n", 0),
        # A bare function call is not a driver's method.
        ("a plain function named tick is not a window's", "tick(16)\n", 0),
    ]
    failed = 0
    for name, source, want in cases:
        got = _selftest_counts(source)
        if got != want:
            failed += 1
            print(f"FAIL: {name}: want {want}, got {got}", file=sys.stderr)
    # And the budget must describe the tree it is checked in.
    missing = [p for p in read_budget() if not (ROOT / p).exists()]
    if missing:
        failed += 1
        print(f"FAIL: the budget names files that are gone: {missing}", file=sys.stderr)
    print(f"tick_units selftest: {len(cases) - failed} of {len(cases)} cases OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="ratchet check")
    parser.add_argument("--write-budget", action="store_true")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--list", action="store_true", help="every site, with its value")
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.write_budget:
        counts = census()
        write_budget(counts)
        print(
            f"wrote {BUDGET.relative_to(ROOT)}: {len(counts)} file(s), "
            f"{sum(counts.values())} site(s)"
        )
        return 0
    if args.list:
        for path, n in sorted(census().items()):
            print(f"{path}\t{n}")
            for line, seconds in sites(ROOT / path):
                print(f"    {line}: tick({seconds:g})")
        return 0
    return check()


if __name__ == "__main__":
    sys.exit(main())
