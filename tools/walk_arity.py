#!/usr/bin/env python3
"""R2216 — a walk may not CALL a helper with a shape its definition refuses.

## The hole this closes

R2213 made the tree read its walks as Python for the first time
(`walk_names.py`): a name read but never bound. This is the other half of the
same sentence, and R2215 paid for it.

That round threaded a path list through the helpers that spell a path, with a
bulk replace of `value(tf, 1)` -> `value(tf, gp, 1)`. It also rewrote
`open_value(tf, 1)`, because one call name is a SUFFIX of the other. The name
is bound, the file compiles, `walk_names.py` is satisfied — and the call raises
`TypeError: open_value() takes 2 positional arguments but 3 were given` on the
line that runs it. The sweep caught that one because the line runs.

**Nothing catches it when the line does not run.** R2213's own first find was
exactly such a line: two `ok(...)` calls inside a loop over a list that is
empty in this tree, green in the sweep for as long as it stayed empty. A
conversion campaign that rewrites call sites in bulk produces this class, so
the class gets a gate rather than a promise to be careful.

## What it checks

Every call of a MODULE-LEVEL function, in the file that defines it, against
that definition: too many positionals, too few, or a keyword the definition
does not take.

## What it deliberately skips, and why each would otherwise lie

  * `*args` / `**kwargs` in the definition — they accept what this cannot
    count. ★ Measured first: a draft without them reported 86 mismatches in
    this tree, 26 of them `chord(ta, key, **mods)` called as
    `chord(ta, "End", ctrl=True)`. Correct code, every one. A gate that cries
    wolf 86 times is a gate nobody reads, so the first run of the real rule is
    the one that had to be believable.
  * `*splat` / `**splat` at the CALL — the count is not knowable statically.
  * a DECORATED definition — a decorator may return something with another
    signature.
  * a name the calling scope rebinds (a parameter or local of the same name),
    and a name the module assigns as well as defs — the call may not be the
    def's.
  * anything imported. This reads one file at a time; a helper from another
    module is that module's contract, and guessing across files is how a
    checker starts inventing.

Usage:
    python3 tools/walk_arity.py --check      # the gate (pre-push)
    python3 tools/walk_arity.py --selftest   # its own tests
    python3 tools/walk_arity.py --list       # every file, with its count
"""

from __future__ import annotations

import argparse
import ast
import sys
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, str(Path(__file__).resolve().parent))

from walk_names import SCOPE_NODES, binds, own_nodes, sources  # noqa: E402


class Shape(NamedTuple):
    """What a definition accepts."""

    required: int  # positionals with no default
    allowed: int  # positionals in total
    star: bool  # takes *args
    starstar: bool  # takes **kwargs
    keywords: frozenset[str]  # names callable by keyword
    required_kw: frozenset[str]  # keyword-only with no default


class Mismatch(NamedTuple):
    path: str
    line: int
    name: str
    said: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line} calls {self.name}() {self.said}"


def shape_of(node: ast.FunctionDef | ast.AsyncFunctionDef) -> Shape:
    a = node.args
    positional = [*a.posonlyargs, *a.args]
    required = len(positional) - len(a.defaults)
    keywords = {arg.arg for arg in a.args} | {arg.arg for arg in a.kwonlyargs}
    required_kw = {
        arg.arg for arg, default in zip(a.kwonlyargs, a.kw_defaults) if default is None
    }
    return Shape(
        required=required,
        allowed=len(positional),
        star=a.vararg is not None,
        starstar=a.kwarg is not None,
        keywords=frozenset(keywords),
        required_kw=frozenset(required_kw),
    )


def module_defs(tree: ast.Module) -> dict[str, Shape]:
    """Module-level, undecorated `def`s whose name the module binds ONCE."""
    seen: dict[str, int] = {}
    shapes: dict[str, Shape] = {}
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            seen[node.name] = seen.get(node.name, 0) + 1
            if not node.decorator_list:
                shapes[node.name] = shape_of(node)
    # A name the module also ASSIGNS, or defines twice, may not be this def at
    # the call — drop it rather than guess which one runs.
    assigned: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            for target in node.targets:
                for inner in ast.walk(target):
                    if isinstance(inner, ast.Name):
                        assigned.add(inner.id)
        elif isinstance(node, (ast.Import, ast.ImportFrom)):
            for alias in node.names:
                assigned.add(alias.asname or alias.name.split(".")[0])
        elif isinstance(node, ast.ClassDef):
            assigned.add(node.name)
    return {
        name: shape
        for name, shape in shapes.items()
        if seen[name] == 1 and name not in assigned
    }


def _fault(shape: Shape, call: ast.Call) -> str | None:
    """Why `call` does not fit `shape`, or None if it does."""
    if any(isinstance(arg, ast.Starred) for arg in call.args):
        return None
    if any(kw.arg is None for kw in call.keywords):
        return None
    given = len(call.args)
    by_keyword = {kw.arg for kw in call.keywords if kw.arg}
    if not shape.starstar:
        unknown = by_keyword - shape.keywords
        if unknown:
            return f"with keyword(s) {sorted(unknown)} the definition does not take"
    if not shape.star and given > shape.allowed:
        return f"with {given} positional argument(s); the definition takes at most {shape.allowed}"
    filled = given + len(by_keyword & shape.keywords)
    if filled < shape.required:
        return (
            f"with {given} positional + {sorted(by_keyword)}; the definition "
            f"requires {shape.required}"
        )
    missing = shape.required_kw - by_keyword
    if missing and not shape.starstar:
        return f"without required keyword-only argument(s) {sorted(missing)}"
    return None


def mismatches_in(path: Path, text: str | None = None) -> list[Mismatch]:
    """Every call in `path` that its own file's definition would refuse."""
    source = path.read_text(encoding="utf-8") if text is None else text
    tree = ast.parse(source, str(path))
    known = module_defs(tree)
    if not known:
        return []
    name = str(path)
    found: list[Mismatch] = []

    def visit(scope: ast.AST, shadowed: frozenset[str]) -> None:
        # A scope that binds a helper's name may be calling its own thing —
        # except the MODULE, whose binding of that name IS the definition.
        mine = frozenset() if isinstance(scope, ast.Module) else frozenset(binds(scope))
        hidden = shadowed | mine
        nested: list[ast.AST] = []
        for node in own_nodes(scope):
            if isinstance(node, SCOPE_NODES):
                nested.append(node)
                continue
            if (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Name)
                and node.func.id in known
                and node.func.id not in hidden
            ):
                said = _fault(known[node.func.id], node)
                if said is not None:
                    found.append(Mismatch(name, node.lineno, node.func.id, said))
        for inner in nested:
            visit(inner, hidden)

    visit(tree, frozenset())
    return found


def scan() -> list[Mismatch]:
    found: list[Mismatch] = []
    for path in sources():
        rel = str(path.relative_to(Path(__file__).resolve().parent.parent))
        try:
            source = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        try:
            rows = mismatches_in(path, source)
        except SyntaxError as exc:
            found.append(Mismatch(rel, exc.lineno or 0, "<parse>", str(exc)))
            continue
        found.extend(Mismatch(rel, row.line, row.name, row.said) for row in rows)
    return found


def check() -> int:
    found = scan()
    population = len(sources())
    if found:
        print(f"walk-arity: {len(found)} call(s) a definition refuses, over {population} file(s):")
        for row in found:
            print(f"walk-arity:   {row}")
        print("walk-arity: each is a TypeError the moment that line runs — the sweep")
        print("walk-arity: would only find it if the line is reached.")
        return 1
    print(f"walk-arity: every call of a file's own helper fits it, over {population} file(s)")
    return 0


def listing() -> int:
    found = scan()
    for path in sources():
        rel = str(path.relative_to(Path(__file__).resolve().parent.parent))
        hits = sum(1 for row in found if row.path == rel)
        print(f"{hits:3}  {rel}")
    print(f"walk-arity: {len(found)} mismatch(es) over {len(sources())} file(s)")
    return 0


CASES: tuple[tuple[str, str, int], ...] = (
    (
        "★ the R2215 defect: a suffix collision gave one argument too many",
        """
def value(tf, gp, nid):
    return tf.query(gp.at("node.value", id=nid))


def open_value(tf, nid):
    return tf.click(nid)


def body(tf, gp):
    open_value(tf, gp, 1)
    return value(tf, gp, 1)
""",
        1,
    ),
    (
        "a call with too few arguments",
        """
def place(tf, gp, nid, x, y):
    return tf.intervene(gp.at("node.x", id=nid), x + y)


def body(tf, gp):
    return place(tf, gp, 0)
""",
        1,
    ),
    (
        "a keyword the definition does not take",
        """
def scrub(tf, row, col):
    return tf.drag(row, col)


def body(tf):
    return scrub(tf, 0, colour=2)
""",
        1,
    ),
    (
        "★ a definition with **kwargs accepts any keyword — 26 false alarms once",
        """
def chord(ta, key, **mods):
    return ta.invoke("/external/key", {"key": key, **mods})


def body(ta):
    return chord(ta, "End", ctrl=True)
""",
        0,
    ),
    (
        "a definition with *args accepts any count",
        """
def press(tf, *keys):
    return [tf.key(k) for k in keys]


def body(tf):
    return press(tf, "a", "b", "c")
""",
        0,
    ),
    (
        "defaults make trailing positionals optional",
        """
def wait(tf, timeout=4.0, interval=0.03):
    return tf.wait(timeout, interval)


def body(tf):
    return wait(tf)
""",
        0,
    ),
    (
        "a positional passed by keyword still fills it",
        """
def place(tf, nid, x):
    return tf.intervene(nid, x)


def body(tf):
    return place(tf, nid=0, x=40)
""",
        0,
    ),
    (
        "a required keyword-only argument must be given",
        """
def q(tf, *, path):
    return tf.query(path)


def body(tf):
    return q(tf)
""",
        1,
    ),
    (
        "a keyword-only argument WITH a default is optional",
        """
def q(tf, *, path="/external"):
    return tf.query(path)


def body(tf):
    return q(tf)
""",
        0,
    ),
    (
        "a splat at the call is not counted",
        """
def place(tf, nid, x, y):
    return tf.intervene(nid, x, y)


def body(tf, rows):
    return place(tf, *rows)
""",
        0,
    ),
    (
        "a DECORATED definition is skipped — the decorator may reshape it",
        """
import functools


@functools.cache
def pos(tf, nid):
    return tf.query(nid)


def body(tf):
    return pos(tf, 0, 1)
""",
        0,
    ),
    (
        "a local of the same name is not the module's helper",
        """
def pos(tf, nid):
    return tf.query(nid)


def body(tf, pos):
    return pos(tf, 1, 2, 3)
""",
        0,
    ),
    (
        "a name the module also assigns is not necessarily the def",
        """
def pos(tf, nid):
    return tf.query(nid)


pos = None


def body(tf):
    return pos(tf, 1, 2, 3)
""",
        0,
    ),
    (
        "a method call of the same name is not this helper",
        """
def query(tf, path):
    return tf.query(path)


def body(tf):
    return tf.query("a", "b", "c")
""",
        0,
    ),
    (
        "a nested call inside a lambda is checked too",
        """
def pos(tf, gp, nid):
    return tf.query(gp.at("node.x", id=nid))


def body(tf, gp):
    return sorted([0, 1], key=lambda n: pos(tf, n))
""",
        1,
    ),
)


def selftest() -> int:
    failures = 0
    for label, source, expected in CASES:
        got = len(mismatches_in(Path("<case>"), source))
        if got != expected:
            failures += 1
            print(f"walk-arity selftest FAIL: {label}")
            print(f"  expected {expected} mismatch(es), got {got}")
    if len(sources()) < 100:
        failures += 1
        print(f"walk-arity selftest FAIL: population is {len(sources())} file(s)")
    total = len(CASES) + 1
    print(f"walk-arity selftest: {total - failures} of {total} cases OK")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description="a walk may not call a helper wrongly")
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
