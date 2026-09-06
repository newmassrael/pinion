#!/usr/bin/env python3
"""R2028 — **every pure rule's ORACLE is exercised against the real world.**

    python3 tools/oracle_census.py            # the census; exits 1 on a gap
    python3 tools/oracle_census.py --selftest # this tool's own tests

# What this counts, and why it is not a list

`debt-a-pure-rules-oracle-is-a-second-gate-nobody-tests` records the shape.
A gate tool here is built out of PURE rules — functions that take their world
as arguments, so the tool's `--selftest` can hand them fixtures and check the
rule without a toolchain or a filesystem. That is the design's strength and it
has one hole: **a pure rule does not decide what it looks at.** The ORACLE
decides, the oracle is impure by construction, and a fixture cannot reach it.

Measured at R1884 on one such pair: two mutations of `store_sections` — adding
`alternatives_rejected` to the decision text, and dropping `caveats_bullets` —
left `analyzer_census.py --selftest` AND `--check-pin` green while making a
sentence the specification REJECTED citable as a ratified boundary. R1884
repaired that one site and wrote down that it had not counted the class.

⇒ this is the count, as a command. The population is DERIVED from the source
rather than listed here, which is the same rule the tools it scans keep: a
hand-written list of gate sites is a second census that goes stale, and this
file would be the place it went stale in.

# The derivation

For each scanned tool, read its module with `ast` and split its top-level
functions in two:

* **impure** — the body mentions the world (`read_text`, `exists`, `glob`,
  `open`, `subprocess`, …). These are the oracles.
* **pure** — takes at least one argument and mentions none of it. These are
  the rules.

Then, at every call of a pure rule anywhere in the tool, an argument that is
(a) a call to an impure function, (b) a local assigned from one, or (c) an
impure lambda, IS that rule's oracle at that site. The oracle is SATISFIED
when the tool's own `selftest` calls it by name.

⚠ An inline lambda can never be satisfied, and that is not a technicality: it
has no name, so no assertion can reach it and no mutation can be aimed at it.
Naming it is the repair, which is why the census reports it as a gap rather
than skipping it.

# ⚠ What this census cannot see, stated rather than discovered later

* A tool whose rules take their world through a module-level constant instead
  of an argument has no oracle to find, and reads as clean here.
* Purity is judged by TEXT, not by tracing calls. A "pure" rule that calls an
  impure helper by name is not pure and this will not say so.
* The satisfaction test is *`selftest` calls the oracle*. It cannot check that
  the call ASSERTS anything useful — an oracle called and ignored passes. That
  is a proxy, and it is the same proxy `--check-proofs` uses one level up
  (a citation the runner can select is not a citation that proves anything).

Each of those is a place a defect can sit, which is why the count is printed
even when it is zero: a census that only speaks when it fires is one nobody
can tell from a census that stopped running.
"""

from __future__ import annotations

import ast
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The tools this census covers — every gate tool in `tools/` that has a
#: `selftest`, derived at run time rather than listed, so a new one joins the
#: population by existing rather than by somebody remembering.
def scanned() -> list[pathlib.Path]:
    out = []
    for path in sorted((ROOT / "tools").glob("*.py")):
        if path.name == pathlib.Path(__file__).name:
            # The census is not a subject of itself: its own rules take their
            # world as arguments precisely so this file's tests can reach them,
            # and counting them would be the tool satisfying its own census.
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        if "def selftest" in text:
            out.append(path)
    return out


#: Words whose presence in a function body means it reads the world.
WORLD = (
    ".read_text",
    ".read_bytes",
    ".exists",
    ".glob",
    ".rglob",
    ".iterdir",
    ".is_dir",
    ".is_file",
    "subprocess",
    "open(",
)


def touches_world(source: str) -> bool:
    """Whether this source text reads the world.

    Pure in its argument, so the selftest can put both answers to it — which is
    the whole point of the split this file is about.
    """
    return any(word in source for word in WORLD)


def oracles(source: str) -> list[tuple[str, str, str, bool]]:
    """Every `(host, rule, oracle, satisfied)` this module's source holds.

    Pure in `source`. The impure half — finding the files and reading them —
    is [`scanned`], which is this tool's own oracle and is called by its
    selftest, because a census of unexercised oracles that did not exercise its
    own would be the joke this repository keeps almost telling.
    """
    tree = ast.parse(source)
    funcs = {n.name: n for n in tree.body if isinstance(n, ast.FunctionDef)}

    def body_of(node: ast.AST) -> str:
        return ast.get_source_segment(source, node) or ""

    impure = {name for name, fn in funcs.items() if touches_world(body_of(fn))}
    pure = {
        name
        for name, fn in funcs.items()
        if fn.args.args and not touches_world(body_of(fn))
    }

    called_in_selftest: set[str] = set()
    if "selftest" in funcs:
        for node in ast.walk(funcs["selftest"]):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
                called_in_selftest.add(node.func.id)

    found: set[tuple[str, str, str, bool]] = set()
    for host, fn in funcs.items():
        if host == "selftest":
            continue
        for node in ast.walk(fn):
            if not (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)):
                continue
            rule = node.func.id
            if rule not in pure:
                continue
            for index, arg in enumerate(node.args):
                oracle = _oracle_of(arg, fn, impure, source)
                if oracle is None:
                    continue
                found.add((host, f"{rule}(arg{index})", oracle, oracle in called_in_selftest))
    return sorted(found)


def _oracle_of(
    arg: ast.expr, host: ast.FunctionDef, impure: set[str], source: str
) -> str | None:
    """The impure producer behind one argument, or `None` when there is none."""
    if isinstance(arg, ast.Lambda):
        return "<lambda>" if touches_world(ast.get_source_segment(source, arg) or "") else None
    if isinstance(arg, ast.Call) and isinstance(arg.func, ast.Name):
        return arg.func.id if arg.func.id in impure else None
    if isinstance(arg, ast.Name):
        # A local, resolved to whatever assigned it in this same function.
        for node in ast.walk(host):
            if (
                isinstance(node, ast.Assign)
                and any(isinstance(t, ast.Name) and t.id == arg.id for t in node.targets)
                and isinstance(node.value, ast.Call)
                and isinstance(node.value.func, ast.Name)
                and node.value.func.id in impure
            ):
                return node.value.func.id
    return None


def report() -> int:
    files = scanned()
    if not files:
        print("oracle census: no gate tool found — the census read nothing", file=sys.stderr)
        return 1
    gaps: list[str] = []
    pairs = 0
    for path in files:
        rows = oracles(path.read_text(encoding="utf-8"))
        pairs += len(rows)
        for host, rule, oracle, satisfied in rows:
            if satisfied:
                continue
            where = f"{path.relative_to(ROOT)}: {host} -> {rule}"
            gaps.append(
                f"{where} takes its world from {oracle!r}, which the tool's own "
                "selftest never calls"
                + (
                    " — an inline lambda has no name, so no assertion can reach "
                    "it and no mutation can be aimed at it: give it one"
                    if oracle == "<lambda>"
                    else ""
                )
            )
    print(
        f"oracle census: {pairs} pure-rule/oracle pair(s) over {len(files)} tool(s), "
        f"{len(gaps)} unexercised"
    )
    for gap in sorted(gaps):
        print(f"oracle census: {gap}", file=sys.stderr)
    return 1 if gaps else 0


def selftest() -> int:
    failures = 0

    def check(name: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"FAIL: {name}")

    check("a body that reads a file touches the world", touches_world("p.read_text()"))
    check("a body that shells out touches the world", touches_world("subprocess.run(x)"))
    check("arithmetic does not", not touches_world("return a + b"))

    # ★ A pure rule fed a named impure oracle the selftest never calls: the
    # shape R1884 measured, written out so this census has a case it must catch.
    unexercised = '''
def look() -> str:
    return P.read_text()

def rule(rows, seen) -> list:
    return [r for r in rows if r in seen]

def main() -> int:
    return rule([], look())

def selftest() -> int:
    return 0 if rule([1], [1]) == [1] else 1
'''
    rows = oracles(unexercised)
    check("the pair is found", [r[2] for r in rows] == ["look"])
    check("and it is a gap", rows and not rows[0][3])

    # ★ The same tool, with the oracle called in its selftest.
    exercised = unexercised.replace(
        "    return 0 if rule([1], [1]) == [1] else 1",
        "    return 0 if rule([1], [1]) == [1] and look() else 1",
    )
    rows = oracles(exercised)
    check("naming it in selftest satisfies it", rows and rows[0][3])

    # ★★ An inline impure lambda is a gap that cannot be closed without naming
    # it — the assertion that makes the census refuse a shape rather than a
    # name.
    lambda_oracle = '''
def rule(rows, exists) -> list:
    return [r for r in rows if exists(r)]

def main() -> int:
    return rule([], lambda p: (P / p).exists())

def selftest() -> int:
    return 0
'''
    rows = oracles(lambda_oracle)
    check("an impure lambda is an oracle", [r[2] for r in rows] == ["<lambda>"])
    check("and it is never satisfied", rows and not rows[0][3])

    # ★ A pure argument is not an oracle: a census that counted every argument
    # would report noise and stop being read.
    pure_arg = '''
def rule(rows, limit) -> list:
    return rows[:limit]

def main() -> int:
    return rule([], 3)

def selftest() -> int:
    return 0
'''
    check("a literal is not an oracle", oracles(pure_arg) == [])

    # ★★★★★ THE CENSUS EXERCISES ITS OWN ORACLE, which is the rule it enforces
    # on everything else. `scanned` reads the world; if it answered nothing this
    # tool would report a clean count over an empty population for ever.
    files = scanned()
    check("the census finds gate tools to read", len(files) >= 3)
    check(
        "and every one of them is a tool with a selftest",
        all("def selftest" in p.read_text(encoding="utf-8") for p in files),
    )
    check(
        "and it does not scan itself",
        pathlib.Path(__file__).resolve() not in {p.resolve() for p in files},
    )

    print("oracle census selftest:", "OK" if not failures else f"{failures} FAILURE(S)")
    return 1 if failures else 0


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()
    return report()


if __name__ == "__main__":
    sys.exit(main())
