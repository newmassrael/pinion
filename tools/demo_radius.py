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


def named_files(path: Path) -> set[str]:
    """Every tracked file this demo names, as a string literal, by parsing it.

    ★★★★★ R1858 — the axis the package radius cannot supply. A tracked PIN
    (`docs/analyzer-*-spec.json`, a budget `.tsv`) is data that a demo asserts
    against, and editing it changes what that demo should say. `blast_radius.py`
    answers *packages*, so a pin reaches the demo set only through the crate
    that `include_str!`s it — which works until the same round also touches
    `pinion-core`, and then the answer is EVERYTHING and the pin's own two
    consumers vanish into six hundred names nobody can run.

    Measured on R1852 (`c77fccf0`), the round that produced this: the package
    axis answered **639** demos, `r1747` among them, and the round narrowed by
    hand and lost it — while `docs/analyzer-packets-spec.json` is named by
    exactly **two**. The narrow set is the one that survives.

    Parsed rather than grepped, and the difference is COMMENTS: a `#` line is
    not in the tree at all, so a path mentioned in one cannot select a demo.
    Docstrings ARE string constants and are deliberately kept — measured while
    this was being built, the two demos that name this round's pin do it as a
    dict value (`"analyzer-packets-spec.json"`) and inside a longer assertion
    message (`"canon in docs/analyzer-packets-spec.json, read off disk ..."`),
    and a rule that demanded a bare literal equal to the path found NEITHER.
    The first draft did exactly that and its own selftest said so.
    """
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (SyntaxError, UnicodeDecodeError):
        return set()
    return {
        node.value
        for node in ast.walk(tree)
        if isinstance(node, ast.Constant) and isinstance(node.value, str)
    }


def demos_naming(paths: list[str]) -> list[tuple[Path, set[str]]]:
    """The demos that name one of `paths`, by its repo-relative path or its
    basename.

    The BASENAME is what is matched, and as a substring of any string constant:
    a demo names a pin as a dict value, inside a sentence it asserts with, or as
    a full path, and all three are claims about that file. A basename like
    `analyzer-packets-spec.json` is specific enough that a substring match is
    not a coincidence — which is why the match is on it rather than on the
    directory.
    """
    wanted = {Path(repo_path).name: repo_path for repo_path in paths}
    out = []
    for demo in sorted(DEMOS.glob("*.py")):
        text = named_files(demo)
        hit = {
            repo_path
            for base, repo_path in wanted.items()
            if any(base in said for said in text)
        }
        if hit:
            out.append((demo, hit))
    return out


def tracked_data(paths: list[str]) -> list[str]:
    """The changed paths that are DATA a demo can assert against.

    Deliberately narrow: files under `docs/` that are not prose. A `.md` is read
    by people, and the atomic store is mutated through its own primitives and
    asserted about by nothing here.
    """
    return [
        p
        for p in paths
        if p.startswith("docs/")
        and p.endswith((".json", ".tsv"))
        and not p.startswith("docs/.atomic/")
    ]


def mentions_launcher(path: Path) -> bool:
    """Whether the file refers to the launcher at all."""
    try:
        return "RpcSubprocess" in path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return False


def changed_paths(mode: str, rev_range: str | None) -> list[str]:
    """The paths this change touches — the same derivation `blast_radius.py`
    uses, asked here because the pin axis needs the PATHS and that tool answers
    package names."""
    if mode == "staged":
        argv = ["git", "diff", "--cached", "--name-only"]
    else:
        assert rev_range, "range mode needs a revision range"
        argv = ["git", "diff", "--name-only", rev_range]
    done = subprocess.run(argv, cwd=REPO, capture_output=True, text=True, check=True)
    return [line.strip() for line in done.stdout.splitlines() if line.strip()]


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
    # ★★★★★ R1858 — the pin axis, checked against files that exist rather than
    # against a story about them.
    #
    # A path in a COMMENT must not count, which is the property that separates
    # this from a grep and the reason it parses.
    # ⚠ Through `named_files` ITSELF and not through a private `ast.parse`
    # beside it. The first draft asserted on a probe it parsed here, so a
    # counterfactual that made `named_files` fall back to the raw file text —
    # the exact grep behaviour this is supposed to differ from — passed. A test
    # that does not call the thing it is about is testing its own copy.
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        probe_path = Path(tmp) / "probe_demo.py"
        probe_path.write_text('# "docs/analyzer-packets-spec.json"\nx = 1\n', encoding="utf-8")
        if any("analyzer-packets-spec.json" in said for said in named_files(probe_path)):
            failures.append("a path inside a comment was read as a reference")

        # And the SUBSTRING property, which is the one that decides whether this
        # axis finds anything at all. Asserted as a comparison rather than as a
        # count: a demo may name the pin as a bare literal or inside a sentence
        # it asserts with, and only the second needs the substring rule — so the
        # substring answer must be STRICTLY larger than the equality answer, or
        # the rule is carrying no weight in this tree.
        embedded = Path(tmp) / "embedded_demo.py"
        embedded.write_text(
            'ok("canon in docs/analyzer-packets-spec.json, read off disk", True)\n',
            encoding="utf-8",
        )
        if not any(
            "analyzer-packets-spec.json" in said for said in named_files(embedded)
        ):
            failures.append("a name inside a longer string was not seen")

    pin = "docs/analyzer-packets-spec.json"
    by_substring = {p for p, _ in demos_naming([pin])}
    by_equality = {
        demo
        for demo in DEMOS.glob("*.py")
        if any(said == Path(pin).name or said == pin for said in named_files(demo))
    }
    if not by_substring > by_equality:
        failures.append(
            "the substring rule finds no demo an equality rule would miss, so it "
            f"is carrying nothing: {len(by_substring)} vs {len(by_equality)}"
        )

    # `tracked_data` must take pins and leave prose and the store alone.
    kept = tracked_data(
        [
            "docs/analyzer-packets-spec.json",
            "docs/phase-b-rounds.tsv",
            "docs/SEED_PROMPT.md",
            "docs/.atomic/workspace.atomic.json",
            "crates/pinion-core/src/lib.rs",
        ]
    )
    if kept != ["docs/analyzer-packets-spec.json", "docs/phase-b-rounds.tsv"]:
        failures.append(f"tracked_data selected {kept!r}")

    # And the axis must actually FIND something in this tree, or it is a switch
    # that reports nothing whatever anybody edits. The pin below is the one the
    # round that built this was repaying; the assertion is that SOME demo names
    # it, not how many, because a count in a test goes stale like a count in
    # prose.
    named = demos_naming(["docs/analyzer-packets-spec.json"])
    if not named:
        failures.append("no demo names docs/analyzer-packets-spec.json")

    for line in failures:
        print(f"demo radius selftest: {line}", file=sys.stderr)
    print(f"demo radius selftest: {len(resolved)} demo(s) resolve a launch target")
    print(
        f"demo radius selftest: {len(named)} demo(s) name the pin the pin axis "
        "was built for"
    )
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["staged", "range"], default="staged")
    parser.add_argument("--range", dest="rev_range")
    parser.add_argument("--command", action="store_true", help="print the run lines")
    parser.add_argument("--count", action="store_true", help="print only how many")
    parser.add_argument("--audit", action="store_true", help="report unresolved launches")
    parser.add_argument(
        "--pins",
        action="store_true",
        help="only the demos that assert against tracked data this change edits",
    )
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

    changed = changed_paths(args.mode, args.rev_range)
    pins = tracked_data(changed)
    by_pin = demos_naming(pins)

    if args.pins:
        # ★ The narrow axis on its own, which is the whole point of it: this
        # answer stays small when the package answer is everything.
        if args.count:
            print(len(by_pin))
            return 0
        for path, hit in by_pin:
            rel = path.relative_to(REPO)
            if args.command:
                print(f"python3 {rel}")
            else:
                print(f"{rel}  ({', '.join(sorted(hit))})")
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
    # ★★★★★ R1858 — and the pin axis is REPORTED beside the package one rather
    # than folded into it. Folding would hide it exactly when it matters: the
    # sets are the same size only while the package answer is small, and the
    # case this exists for is the one where that answer is six hundred.
    if by_pin:
        print(
            f"\n★ {len(by_pin)} of these assert against data this change edits "
            f"({', '.join(pins)}) — run these whatever you narrow the rest to:",
            file=sys.stderr,
        )
        for path, hit in by_pin:
            print(
                f"  python3 {path.relative_to(REPO)}  ({', '.join(sorted(hit))})",
                file=sys.stderr,
            )
    elif pins:
        print(
            f"\n★ this change edits {len(pins)} tracked data file(s) and NO demo "
            f"names any of them: {', '.join(pins)}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
