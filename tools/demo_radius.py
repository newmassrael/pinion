#!/usr/bin/env python3
"""R1797 — which DEMOS a change can break, computed rather than chosen by hand.

## The round that paid for this

R1795 was a whole round spent repaying a CI red that R1791 and R1792 had
published, and its own entry states the cause in one sentence: *the blast radius
was chosen BY HAND, writing down the demos that looked affected each round, and
six of the 645 were not on that list.* It then registered the missing
computation, wrote down its shape, and did not build it -- so the very next
round that touched a screen would pick by hand again with the same odds.

`blast_radius.py` beside this answers **which packages** a change can break, from
`cargo metadata`. Nothing answered **which demos**, and a demo is where a wire
claim is actually asserted: a package's unit tests can be entirely green while a
demo that drives the same binary over RPC fails.

## What it answers

Given a set of changed paths, the demo scripts that launch a binary the change
can reach. Two steps, both derived:

1. `blast_radius.py` gives the workspace packages whose behaviour can change.
2. Every demo under `tools/demos/` names the binary it launches, in the source,
   as `RpcSubprocess("<name>")`. That name is a package name. Read by **parsing
   the file**, not by grepping it: a name inside a comment or a docstring is not
   a launch, and a regex cannot tell the difference. (`tick_units.py` learnt the
   same thing at R1783 -- an unbounded substring search credits somebody else's
   work.)

The intersection is the answer.

## What it deliberately does not do

Decide. It prints the demos and, with `--command`, the lines that run them.
Whether that many is affordable is the caller's judgment, exactly as
`blast_radius.py` leaves the package set to the caller.

## What it cannot see

A demo that launches a binary through anything other than a literal
`RpcSubprocess("name")` -- a name held in a variable, or a helper that wraps the
launch. `--audit` reports every demo whose launch target could not be resolved,
so that set is a NUMBER a round can look at rather than a silence. Measured at
R1797 it is reported below rather than written here, because a count in prose
goes stale the moment somebody adds a demo.
"""

from __future__ import annotations

import argparse
import ast
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEMOS = REPO / "tools" / "demos"


def module_strings(tree: ast.Module) -> dict[str, str]:
    """Module-level `NAME = "literal"` bindings.

    ★ Measured at R1797, when this tool first ran: **254 of 646 demos** resolved
    no launch target without this, because the dominant shape in this tree is
    `EXAMPLE = "hello-thing"` at the top of the file and `RpcSubprocess(EXAMPLE)`
    below. A radius tool blind to two demos in five is not a computation, it is
    a shorter hand-written list -- which is the thing it exists to replace.

    Module level only, and deliberately: a name rebound inside a function can
    hold a different value at each call site, and following that is an
    interpreter rather than a lookup. Those stay unresolved and `--audit`
    reports them.
    """
    out: dict[str, str] = {}
    for node in tree.body:
        targets = []
        if isinstance(node, ast.Assign):
            targets = node.targets
        elif isinstance(node, ast.AnnAssign) and node.value is not None:
            targets = [node.target]
        else:
            continue
        value = node.value
        if not isinstance(value, ast.Constant) or not isinstance(value.value, str):
            continue
        for target in targets:
            if isinstance(target, ast.Name):
                out[target.id] = value.value
    return out


def launched_by(path: Path) -> set[str]:
    """The package names a demo script launches, by parsing it.

    Every `RpcSubprocess(...)` call in the file, wherever it appears --
    including inside a helper defined in the same file, which is why this walks
    the whole tree rather than only the top level. The first argument resolves
    from a string literal or from a module-level constant.
    """
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (SyntaxError, UnicodeDecodeError):
        return set()
    consts = module_strings(tree)
    out: set[str] = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        func = node.func
        name = func.id if isinstance(func, ast.Name) else getattr(func, "attr", None)
        if name != "RpcSubprocess":
            continue
        if not node.args:
            continue
        first = node.args[0]
        if isinstance(first, ast.Constant) and isinstance(first.value, str):
            out.add(first.value)
        elif isinstance(first, ast.Name) and first.id in consts:
            out.add(consts[first.id])
    return out


def mentions_launcher(path: Path) -> bool:
    """Whether the file refers to the launcher at all."""
    try:
        return "RpcSubprocess" in path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return False


def packages(mode: str, rev_range: str | None) -> list[str]:
    """The package set `blast_radius.py` computes for this change."""
    argv = [sys.executable, str(REPO / "tools" / "blast_radius.py"), "--mode", mode]
    if rev_range:
        argv += ["--range", rev_range]
    done = subprocess.run(argv, capture_output=True, text=True, check=True, cwd=REPO)
    return [line.strip() for line in done.stdout.splitlines() if line.strip()]


def demos_for(names: set[str]) -> list[tuple[Path, set[str]]]:
    out = []
    for path in sorted(DEMOS.glob("*.py")):
        launched = launched_by(path)
        hit = launched & names
        if hit:
            out.append((path, hit))
    return out


def audit() -> list[Path]:
    """Demos that mention the launcher and whose target could not be resolved."""
    return [
        path
        for path in sorted(DEMOS.glob("*.py"))
        if mentions_launcher(path) and not launched_by(path)
    ]


def selftest() -> int:
    """The parse is the claim; check it against files that exist."""
    failures = []
    resolved = [p for p in DEMOS.glob("*.py") if launched_by(p)]
    if not resolved:
        failures.append("no demo resolved a launch target at all")
    # Every resolved name must be a real example directory, or the mapping is
    # matching something that is not a package.
    known = {p.name for p in (REPO / "examples").iterdir() if p.is_dir()}
    for path in resolved:
        for name in launched_by(path):
            if name not in known:
                failures.append(f"{path.name} launches {name!r}, which is not an example")
    # A name in a COMMENT must not be picked up -- the property a grep lacks.
    probe = ast.parse('# RpcSubprocess("not-a-real-example")\nx = 1\n')
    found = [
        n
        for n in ast.walk(probe)
        if isinstance(n, ast.Call) and getattr(n.func, "id", None) == "RpcSubprocess"
    ]
    if found:
        failures.append("a commented-out launch was parsed as a launch")
    for line in failures:
        print(f"demo radius selftest: {line}", file=sys.stderr)
    print(f"demo radius selftest: {len(resolved)} demo(s) resolve a launch target")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["staged", "range"], default="staged")
    parser.add_argument("--range", dest="rev_range")
    parser.add_argument("--command", action="store_true", help="print the run lines")
    parser.add_argument("--count", action="store_true", help="print only how many")
    parser.add_argument("--audit", action="store_true", help="report unresolved launches")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if args.audit:
        unresolved = audit()
        print(f"{len(unresolved)} demo(s) mention the launcher and resolve no target")
        for path in unresolved:
            print(f"  {path.relative_to(REPO)}")
        return 0

    names = set(packages(args.mode, args.rev_range))
    hits = demos_for(names)
    if args.count:
        print(len(hits))
        return 0
    for path, launched in hits:
        rel = path.relative_to(REPO)
        if args.command:
            print(f"python3 {rel}")
        else:
            print(f"{rel}  ({', '.join(sorted(launched))})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
