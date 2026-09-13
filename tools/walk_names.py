#!/usr/bin/env python3
"""R2213 — a walk may not READ a name nothing binds.

## The hole this closes

`tools/demos/*.py` is 700-odd scripts that drive real binaries over RPC — and
`tools/*.py` beside them is every gate this project pushes through — and until
this gate **nothing in the tree read any of it as Python**. No linter is
configured here, none is installed, and no hook or CI job runs one: the
`# noqa: E402` markers the walks carry are vestigial, enforced by nothing. A
name a walk reads but never binds is a `NameError` — at RUNTIME, on the line
that reads it, so the only thing that can notice is the demo sweep, and only
if that line is reached. A walk whose failing branch is a guard nobody trips
can carry the defect indefinitely.

That is not hypothetical here. R2198-R2212 converted ~450 read sites to
compose their paths from what a screen DECLARES:

    gp = external_paths(tf)
    ... gp.at("node.x", id=nid)

and the conversion's own failure mode is exactly this shape — a module-level
helper that spells `gp.at(...)` while nobody hands it `gp`. R2198 wrote an
`ast` check for it as a SCRATCH script, which meant each round re-ran it by
hand and no future round was bound to. R2212 registered that as the debt; this
is its repayment, generalised past one name: a typo'd helper, a dropped
import, a renamed constant and an unhanded `gp` are one defect with one shape.

## What it checks

Every `Name` a file READS resolves to a binding visible where Python would
look for it: the scope it sits in, an enclosing function scope, the module, or
`builtins`. Bindings are collected per scope — parameters, assignments,
`for` / `with` / `except` targets, comprehension targets, walrus, imports,
`def` / `class` names, `global` / `nonlocal` declarations, and match captures.

⚠ SCOPE, not ORDER. Python resolves a global when the line runs, so a helper
defined at the top may legitimately read a constant assigned at the bottom.
This gate therefore asks whether a name is bound ANYWHERE in a visible scope,
never whether the binding comes first. Flagging order would be a stream of
false alarms about correct code, and a gate that cries wolf is one nobody
reads.

⚠ CLASS scopes are not visible to functions nested in them — Python's rule,
and one a simpler reading gets wrong.

★★ R2212 measured why the traversal is written out by hand rather than with
`ast.walk`: `ast.walk` is FLAT, so "skip a nested function" skips the
`FunctionDef` node and still visits its body. A checker built that way credits
every function's assignments to the module, and then reports clean on a helper
that reads a `gp` nobody handed it in a file whose `body()` happens to bind
one — the likeliest real shape of the defect. `--selftest` pins that case.

## What it deliberately does not do

Guess. A file that does `from x import *` has bindings this cannot see, so it
is REPORTED AS SKIPPED rather than read past — a name the star import
provides would otherwise be a false alarm, and a gate is worth what its
quietest failure is worth. `--check` prints the skipped list; there are none
in this tree today, and if one appears the gate says so instead of going
silently blind.

Usage:
    python3 tools/walk_names.py --check      # the gate (pre-push)
    python3 tools/walk_names.py --selftest   # its own tests
    python3 tools/walk_names.py --list       # every file read, with its count
"""

from __future__ import annotations

import argparse
import ast
import builtins
import sys
from pathlib import Path
from typing import Iterator, NamedTuple

ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"
DEMOS = TOOLS / "demos"

BUILTINS = frozenset(dir(builtins)) | {"__file__", "__name__", "__doc__", "__spec__"}

FUNCTION_SCOPES = (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)
COMPREHENSIONS = (ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp)
SCOPE_NODES = (*FUNCTION_SCOPES, *COMPREHENSIONS, ast.ClassDef)


class Unbound(NamedTuple):
    """One read of a name no visible scope binds."""

    path: str
    line: int
    name: str
    holder: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line} reads {self.name!r} in {self.holder}, which nothing binds"


def sources() -> tuple[Path, ...]:
    """The population: every walk, and every tool beside them.

    ⚠ The tools are in it, not only `tools/demos/`. R1783's finding is why:
    the defect that round chased lived in `rpc_verify.py` itself, not only in
    the demos written against it — and every gate in this directory is Python
    that nothing else here reads either. Measured when this widened: the tools
    answer 0, so the population costs nothing today and covers the next typo.
    """
    return (*sorted(DEMOS.glob("*.py")), *sorted(TOOLS.glob("*.py")))


def _target_names(node: ast.AST) -> Iterator[str]:
    """Every name a binding target binds — through tuples, lists and stars."""
    for inner in ast.walk(node):
        if isinstance(inner, ast.Name):
            yield inner.id


def _scope_body(scope: ast.AST) -> list[ast.AST]:
    """The nodes a scope owns directly, before pruning nested scopes."""
    if isinstance(scope, ast.Lambda):
        return [scope.body]
    if isinstance(scope, COMPREHENSIONS):
        parts: list[ast.AST] = [scope.elt] if hasattr(scope, "elt") else []
        if isinstance(scope, ast.DictComp):
            parts = [scope.key, scope.value]
        for gen in scope.generators:
            parts.append(gen.target)
            parts.append(gen.iter)
            parts.extend(gen.ifs)
        return parts
    body = getattr(scope, "body", [])
    return list(body) if isinstance(body, list) else [body]


def own_nodes(scope: ast.AST) -> Iterator[ast.AST]:
    """Every node belonging to `scope` itself, nested scopes pruned WHOLE.

    ⚠ Not `ast.walk`: see the module docstring. `ast.walk` is a flat
    traversal, so skipping a nested scope's node does not skip its body.
    """
    stack = list(_scope_body(scope))
    while stack:
        node = stack.pop()
        yield node
        if isinstance(node, SCOPE_NODES):
            continue  # its body belongs to that scope
        stack.extend(ast.iter_child_nodes(node))


def binds(scope: ast.AST) -> set[str]:
    """The names `scope` binds itself: parameters and every binding form."""
    out: set[str] = set()
    args = getattr(scope, "args", None)
    if isinstance(args, ast.arguments):
        for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs]:
            out.add(arg.arg)
        for extra in (args.vararg, args.kwarg):
            if extra is not None:
                out.add(extra.arg)
    if isinstance(scope, COMPREHENSIONS):
        for gen in scope.generators:
            out.update(_target_names(gen.target))
    for node in own_nodes(scope):
        if isinstance(node, ast.Assign):
            for target in node.targets:
                out.update(_target_names(target))
        elif isinstance(node, (ast.AnnAssign, ast.AugAssign, ast.NamedExpr)):
            out.update(_target_names(node.target))
        elif isinstance(node, (ast.For, ast.AsyncFor)):
            out.update(_target_names(node.target))
        elif isinstance(node, ast.withitem):
            if node.optional_vars is not None:
                out.update(_target_names(node.optional_vars))
        elif isinstance(node, ast.ExceptHandler):
            if node.name:
                out.add(node.name)
        elif isinstance(node, (ast.Import, ast.ImportFrom)):
            for alias in node.names:
                out.add(alias.asname or alias.name.split(".")[0])
        elif isinstance(node, (ast.Global, ast.Nonlocal)):
            out.update(node.names)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            out.add(node.name)
        elif isinstance(node, (ast.MatchAs, ast.MatchStar)):
            if node.name:
                out.add(node.name)
        elif isinstance(node, ast.MatchMapping):
            if node.rest:
                out.add(node.rest)
    return out


def _holder(scope: ast.AST) -> str:
    if isinstance(scope, ast.Module):
        return "<module>"
    if isinstance(scope, ast.Lambda):
        return "<lambda>"
    if isinstance(scope, COMPREHENSIONS):
        return "<comprehension>"
    return getattr(scope, "name", "<scope>")


def star_imported(tree: ast.Module) -> bool:
    """Does this module pull names it cannot enumerate?"""
    return any(
        isinstance(node, ast.ImportFrom) and any(a.name == "*" for a in node.names)
        for node in ast.walk(tree)
    )


def unbound_in(path: Path, text: str | None = None) -> list[Unbound]:
    """Every read in `path` that no visible scope binds."""
    source = path.read_text(encoding="utf-8") if text is None else text
    tree = ast.parse(source, str(path))
    name = str(path)
    found: list[Unbound] = []

    def visit(scope: ast.AST, outer: frozenset[str], through: frozenset[str]) -> None:
        """`outer` is what enclosing FUNCTION scopes offer; `through` is what a
        nested function would see — a class body's names are visible to the
        class body itself and to nobody nested inside it."""
        mine = binds(scope)
        visible = outer | mine
        nested: list[ast.AST] = []
        for node in own_nodes(scope):
            if isinstance(node, SCOPE_NODES):
                nested.append(node)
                continue
            if (
                isinstance(node, ast.Name)
                and isinstance(node.ctx, ast.Load)
                and node.id not in visible
                and node.id not in BUILTINS
            ):
                found.append(Unbound(name, node.lineno, node.id, _holder(scope)))
        inner_through = through if isinstance(scope, ast.ClassDef) else visible
        for node in nested:
            visit(node, inner_through, inner_through)

    module_names = binds(tree)
    visit(tree, frozenset(module_names), frozenset(module_names))
    return found


def scan() -> tuple[list[Unbound], list[str]]:
    """Every unbound read in the population, and the files skipped."""
    found: list[Unbound] = []
    skipped: list[str] = []
    for path in sources():
        rel = str(path.relative_to(ROOT))
        try:
            source = path.read_text(encoding="utf-8")
            tree = ast.parse(source, rel)
        except (SyntaxError, UnicodeDecodeError) as exc:
            found.append(Unbound(rel, getattr(exc, "lineno", 0) or 0, "<parse>", str(exc)))
            continue
        if star_imported(tree):
            skipped.append(rel)
            continue
        found.extend(
            Unbound(rel, row.line, row.name, row.holder) for row in unbound_in(path, source)
        )
    return found, skipped


def check() -> int:
    found, skipped = scan()
    population = len(sources())
    if skipped:
        print(f"walk-names: {len(skipped)} file(s) SKIPPED — a star import hides their bindings:")
        for rel in skipped:
            print(f"walk-names:   {rel}")
    if found:
        print(f"walk-names: {len(found)} read(s) name(s) nothing binds, over {population} file(s):")
        for row in found:
            print(f"walk-names:   {row}")
        print("walk-names: each is a NameError the moment that line runs — the sweep")
        print("walk-names: would only find it if the line is reached.")
        return 1
    print(
        f"walk-names: every name read in {population} walk / tool file(s) "
        f"resolves ({len(skipped)} skipped)"
    )
    return 0


def listing() -> int:
    found, skipped = scan()
    for path in sources():
        rel = str(path.relative_to(ROOT))
        hits = sum(1 for row in found if row.path == rel)
        mark = " SKIPPED" if rel in skipped else ""
        print(f"{hits:3}  {rel}{mark}")
    print(f"walk-names: {len(found)} unbound read(s) over {len(sources())} file(s)")
    return 0


CASES: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    (
        "a nested function closing over its parent's name is LEGAL",
        """
from rpc_verify import external_paths


def body(tf):
    gp = external_paths(tf)

    def place(nid):
        return gp.at("node.x", id=nid)

    return place(0)
""",
        (),
    ),
    (
        "a helper reading a name nobody hands it is caught",
        """
def read_title(tf, nid):
    return tf.query(gp.at("node.title", id=nid))
""",
        ("gp",),
    ),
    (
        "★ and it is caught when another function DOES bind that name",
        """
from rpc_verify import external_paths


def node_x(tf, nid):
    return tf.query(gp.at("node.x", id=nid))


def body(tf):
    gp = external_paths(tf)
    return node_x(tf, 0)
""",
        ("gp",),
    ),
    (
        "a module constant read by a function defined ABOVE it resolves",
        """
def body(tf):
    return tf.query(EXT)


EXT = "/external"
""",
        (),
    ),
    (
        "a comprehension binds its own target",
        """
def body(tf):
    return [eid for eid in tf.ids()]
""",
        (),
    ),
    (
        "a comprehension's target does NOT leak to the enclosing scope",
        """
def body(tf):
    rows = [eid for eid in tf.ids()]
    return rows, eid
""",
        ("eid",),
    ),
    (
        "an except alias binds inside its handler",
        """
def body(tf):
    try:
        return tf.query("x")
    except OSError as exc:
        return str(exc)
""",
        (),
    ),
    (
        "a with-as target binds",
        """
def body():
    with RpcSubprocess("app") as tf:
        return tf.query("x")


from rpc_verify import RpcSubprocess
""",
        (),
    ),
    (
        "a walrus binds",
        """
def body(tf):
    if (found := tf.query("x")) is not None:
        return found
    return None
""",
        (),
    ),
    (
        "a lambda's parameter binds inside it",
        """
def body(tf):
    return sorted(tf.rows(), key=lambda row: row.x)
""",
        (),
    ),
    (
        "a lambda closing over an enclosing name is legal",
        """
def body(tf):
    gp = external_paths(tf)
    return wait_until(lambda: tf.query(gp.at("node.x", id=0)))
""",
        ("external_paths", "wait_until"),
    ),
    (
        "a class body's names are NOT visible to a method (Python's rule)",
        """
class Paths:
    mount = "/external"

    def at(self):
        return mount
""",
        ("mount",),
    ),
    (
        "a global declaration binds",
        """
def body():
    global TOTAL
    TOTAL = 1
    return TOTAL
""",
        (),
    ),
    (
        "a for target binds, including a tuple target",
        """
def body(rows):
    for eid, conn in rows:
        print(eid, conn)
""",
        (),
    ),
    (
        "a builtin is not unbound",
        """
def body(rows):
    return len(list(rows))
""",
        (),
    ),
    (
        "an import binds the name it introduces",
        """
import json


def body(text):
    return json.loads(text)
""",
        (),
    ),
)


def selftest() -> int:
    failures = 0
    for label, source, expected in CASES:
        got = tuple(row.name for row in unbound_in(Path("<case>"), source))
        if tuple(sorted(got)) != tuple(sorted(expected)):
            failures += 1
            print(f"walk-names selftest FAIL: {label}")
            print(f"  expected {sorted(expected)}, got {sorted(got)}")
    # The population is part of the contract: a gate that reads no file passes
    # vacuously, which is the failure this project has been bitten by twice.
    if len(sources()) < 100:
        failures += 1
        print(f"walk-names selftest FAIL: population is {len(sources())} file(s), expected the walks")
    if failures:
        print(f"walk-names selftest: {len(CASES) + 1 - failures} of {len(CASES) + 1} cases OK")
        return 1
    print(f"walk-names selftest: {len(CASES) + 1} of {len(CASES) + 1} cases OK")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="a walk may not read a name nothing binds")
    parser.add_argument("--check", action="store_true", help="the gate")
    parser.add_argument("--selftest", action="store_true", help="its own tests")
    parser.add_argument("--list", action="store_true", help="every file, with its count")
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.list:
        return listing()
    return check()


if __name__ == "__main__":
    sys.exit(main())
