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
`RpcSubprocess("name")` -- a name held in a variable it cannot follow.
`--audit` reports every demo whose launch target could not be resolved, so that
set is a NUMBER a round can look at rather than a silence. Measured at R1797 it
is reported below rather than written here, because a count in prose goes stale
the moment somebody adds a demo.

## R2106 -- and the silence had a cost, measured

`r1700_what_is_drawn_is_what_is_pressed.py` drives THREE screens through a
helper of its own, `screen(example, name)`, called three times with a literal
each. Its launch argument is that helper's PARAMETER, so this tool resolved
nothing and the demo sat in `--audit` -- which meant a change to
`hello-node-lab` never selected the walk that reads that screen's whole
specification back off the paint. R2104 published a red in it; R2105 published
over the red; neither round's radius sweep could have run it, and neither round
was careless. **A blind spot in the instrument reads exactly like a green.**

So the parameter hop is followed now: when a launch argument is the name of a
parameter of the enclosing module-level function, the literals passed at that
position by calls to that function IN THE SAME MODULE resolve it. One hop, not
an interpreter -- `module_strings`'s own rule, applied to arguments instead of
assignments, and it can only ADD targets to a demo that resolved none by the
literal path.

## R2126 -- the same blind spot, in the shape this tree actually writes

R2106 fixed the hop where a demo calls its helper with a LITERAL per screen. The
dominant shape here is the other one: a module-level roster --
`SCREENS = [("node lab", "hello-node-lab"), ...]` -- iterated with tuple
unpacking, and the unpacked name either launched directly or handed to a helper.
Neither hop sees it, so such a demo resolved nothing and sat in `--audit`.

**Measured, and it cost two rounds.** `r1712_a_window_says_what_it_gives_up.py`
drives seven screens off such a roster and asserts, among much else, which of
them publish a specification naming the regions they paint. R2123 made
`hello-key-patterns` publish one and R2124 made `hello-log-view` publish one;
each grew that set, each left the demo's recorded expectation short, and
`--radius` selected the demo for NEITHER round because it could not resolve the
launch. Both published a red that CI found afterwards. Two rounds, one blind
spot, and R2106 had already written down what a blind spot looks like: exactly
like a green.

So a module-level ROSTER is a lookup too. `module_sequences` reads
`NAME = [...]` / `(...)` / `{...}` whose elements are string literals, tuples of
them, or names this module has already bound; `roster_bindings` binds what
`for x in NAME`, `for x, y in NAME`, `for k, v in NAME.items()`, the `.keys()` /
`.values()` views and a destructuring `a, b, c = NAME` put in each target,
scoped to the module-level function the statement sits in; and a call argument
that is one of those names now feeds the R2106 parameter hop the way a literal
does.

Still a lookup rather than an interpreter, and the line is in the same place all
three hops draw it: a name resolves against bindings this module makes at its
top level, never against a value that depends on control flow, on a rebinding,
or on another parameter. What does not resolve stays in `--audit`.
"""

from __future__ import annotations

import argparse
import ast
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple

REPO = Path(__file__).resolve().parent.parent
DEMOS = REPO / "tools" / "demos"


class Roster(NamedTuple):
    """A module-level container of string literals, in the two shapes a `for`
    statement can take it apart.

    `rows` is one entry per element, one component per position: a bare literal
    is a one-component row, `("node lab", "hello-node-lab")` is a two-component
    row, and a dict entry is `(key, value)` so `.items()` unpacks the way Python
    unpacks it. A component that is not a string literal is `None` -- present,
    so positions after it still line up, and unresolvable, so nothing is
    credited to it.

    `bare` is what a bare target binds, which is NOT `rows[i][0]`: iterating a
    list of TUPLES binds the whole tuple, not its first component, and reading
    it as the first component is how a radius starts crediting a demo with a
    screen it never launches. A dict is the one container where the two differ
    legitimately -- bare iteration is its keys -- and that is stated here rather
    than inferred at each use site.
    """

    rows: tuple[tuple[str | None, ...], ...]
    bare: tuple[str | None, ...]
    is_dict: bool


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


def module_sequences(tree: ast.Module, consts: dict[str, str]) -> dict[str, Roster]:
    """Module-level `NAME = [...]` / `(...)` / `{...}` rosters of string literals.

    ★★★★★ R2126 — `module_strings` for a container instead of a scalar, and the
    shape it reads is the one this tree writes far more often than the scalar:
    a roster of screens at the top of a walk, iterated below.

    Elements resolve exactly as far as a lookup goes: a string literal, a tuple
    or list of them (its components, positionally), or a name this module has
    ALREADY bound -- earlier in its own body, which is why the body is walked in
    order and why `*OTHER` splices only a roster defined above it. Anything else
    contributes a `None` component: the row keeps its width, so a later position
    still means what it says, and the unresolved position credits nothing.
    """
    out: dict[str, Roster] = {}

    def component(node: ast.expr) -> str | None:
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return node.value
        if isinstance(node, ast.Name):
            return consts.get(node.id)
        return None

    def roster(value: ast.expr) -> Roster | None:
        if isinstance(value, ast.Dict):
            rows = tuple(
                (component(k) if k is not None else None, component(v))
                for k, v in zip(value.keys, value.values)
            )
            return Roster(rows, tuple(row[0] for row in rows), True)
        if not isinstance(value, (ast.List, ast.Tuple, ast.Set)):
            return None
        rows: list[tuple[str | None, ...]] = []
        bare: list[str | None] = []
        for elt in value.elts:
            if isinstance(elt, ast.Starred):
                spliced = out.get(elt.value.id) if isinstance(elt.value, ast.Name) else None
                if spliced is None:
                    # An unresolvable splice must not be read as "nothing more
                    # in this roster": the elements it would have contributed
                    # are unknown, not absent. One opaque row keeps the roster
                    # honest without inventing a width for them.
                    rows.append((None,))
                    bare.append(None)
                    continue
                rows.extend(spliced.rows)
                bare.extend(spliced.bare)
                continue
            if isinstance(elt, (ast.Tuple, ast.List)):
                rows.append(tuple(component(e) for e in elt.elts))
                bare.append(None)
                continue
            resolved = component(elt)
            rows.append((resolved,))
            bare.append(resolved)
        return Roster(tuple(rows), tuple(bare), False)

    for node in tree.body:
        targets: list[ast.expr] = []
        if isinstance(node, ast.Assign):
            targets = list(node.targets)
        elif isinstance(node, ast.AnnAssign) and node.value is not None:
            targets = [node.target]
        else:
            continue
        built = roster(node.value)
        if built is None:
            continue
        for target in targets:
            if isinstance(target, ast.Name):
                out[target.id] = built
    return out


def enclosing_functions(tree: ast.Module) -> dict[int, str]:
    """Every node's module-level function, by `id`; absent for module scope.

    Shared by the loop hop and the parameter hop, because both resolve a NAME
    against the scope that binds it and "the scope" has to mean the same thing
    to both or one of them credits the other's binding.
    """
    enclosing: dict[int, str] = {}
    for top in tree.body:
        if isinstance(top, (ast.FunctionDef, ast.AsyncFunctionDef)):
            for inner in ast.walk(top):
                enclosing[id(inner)] = top.name
    return enclosing


def roster_bindings(
    tree: ast.Module, rosters: dict[str, Roster], enclosing: dict[int, str]
) -> dict[tuple[str, str], set[str]]:
    """What a module-level roster binds, wherever this module takes one apart.

    ★★★★★ R2126 — keyed by `(scope, name)` for the reason `parameter_arguments`
    is: `example` in one function and `example` in another are different things,
    and pooling them credits a demo with a screen it never launches. The scope
    is the module-level function the statement sits in, or `""` for module level.

    Python takes a roster apart in two places, and both are here because leaving
    one out is how a blind spot survives the round that went looking for it:

    * `for x in NAME`         -- what bare iteration binds (a dict's keys; a
                                 list's elements; nothing for a list of tuples,
                                 because that binds a tuple and not a name)
    * `for a, b in NAME`      -- component `i` of each row, per position
    * `for k, v in NAME.items()`  -- the rows themselves
    * `for x in NAME.keys()` / `.values()` -- component 0 / component 1
    * `a, b, c = NAME`        -- destructuring, which walks the roster's
                                 ELEMENTS rather than a row: `BARE_ROOT =
                                 ("hello-x", "tag", ...)` is one row's worth of
                                 literals spread over several names.

    ⚠ A view a roster cannot legitimately have is refused rather than
    approximated. `.items()` on a list is not a thing; and `for a, b in NAME`
    over a DICT unpacks each KEY, not a key and its value, so it binds nothing
    here -- reading it as a row is the over-selection direction wearing the
    look of an obvious simplification.
    """
    out: dict[tuple[str, str], set[str]] = {}

    def view(iter_node: ast.expr) -> tuple[Roster, str] | None:
        if isinstance(iter_node, ast.Name):
            found = rosters.get(iter_node.id)
            return (found, "bare") if found is not None else None
        if (
            isinstance(iter_node, ast.Call)
            and isinstance(iter_node.func, ast.Attribute)
            and isinstance(iter_node.func.value, ast.Name)
            and not iter_node.args
            and not iter_node.keywords
            and iter_node.func.attr in ("items", "keys", "values")
        ):
            found = rosters.get(iter_node.func.value.id)
            if found is None or not found.is_dict:
                return None
            return found, iter_node.func.attr
        return None

    def bind(scope: str, name: str, values: list[str | None]) -> None:
        kept = {v for v in values if v is not None}
        if kept:
            out.setdefault((scope, name), set()).update(kept)

    def unpack(rows: tuple[tuple[str | None, ...], ...], target: ast.expr, scope: str) -> None:
        if not isinstance(target, (ast.Tuple, ast.List)):
            return
        for index, element in enumerate(target.elts):
            if isinstance(element, ast.Name):
                bind(scope, element.id, [row[index] if len(row) > index else None for row in rows])

    for node in ast.walk(tree):
        if isinstance(node, (ast.For, ast.AsyncFor, ast.comprehension)):
            seen = view(node.iter)
            if seen is None:
                continue
            found, how = seen
            scope = enclosing.get(id(node), "")
            if how in ("keys", "values"):
                # A tuple target here unpacks the key (or the value) ITSELF, and
                # this tool does not model a roster whose entries are tuples of
                # tuples, so only a bare name resolves.
                index = 0 if how == "keys" else 1
                if isinstance(node.target, ast.Name):
                    bind(
                        scope,
                        node.target.id,
                        [row[index] if len(row) > index else None for row in found.rows],
                    )
            elif how == "items":
                unpack(found.rows, node.target, scope)
            elif isinstance(node.target, ast.Name):
                bind(scope, node.target.id, list(found.bare))
            elif not found.is_dict:
                unpack(found.rows, node.target, scope)
            continue
        # `a, b, c = NAME` -- the other place a roster comes apart.
        if not isinstance(node, ast.Assign) or not isinstance(node.value, ast.Name):
            continue
        found = rosters.get(node.value.id)
        if found is None:
            continue
        scope = enclosing.get(id(node), "")
        for target in node.targets:
            if not isinstance(target, (ast.Tuple, ast.List)):
                continue
            for index, element in enumerate(target.elts):
                if isinstance(element, ast.Name) and index < len(found.bare):
                    bind(scope, element.id, [found.bare[index]])
    return out


def parameter_arguments(
    tree: ast.Module,
    consts: dict[str, str] | None = None,
    loops: dict[tuple[str, str], set[str]] | None = None,
    enclosing: dict[int, str] | None = None,
) -> dict[tuple[str, str], set[str]]:
    """For each module-level function's parameter, the string literals this
    module passes at that position.

    ★★★★★ R2106 — the ONE hop that `module_strings` is for assignments. A demo
    that drives several screens writes a helper — `screen(example, name)` — and
    calls it once per screen with a literal each; the launch argument is then
    that helper's PARAMETER, which the literal path cannot see and the constant
    path cannot either.

    Keyed by `(function, parameter)` rather than by parameter name alone,
    because `example` in one helper and `example` in another are different
    things and merging them would credit a demo with a screen it never launches
    — the over-selection direction, which is the one that makes a radius useless
    rather than merely short.

    ⚠ Module-level `def` only, positional and keyword calls. A parameter fed
    from another parameter is not followed: that is the second hop, and
    following it is an interpreter rather than a lookup — `module_strings` drew
    the same line and said so.

    ★★★★★ R2126 — an argument may also be a NAME this module has bound: a
    module-level constant, or a `for` target over a module-level roster. That is
    the shape `SCREENS` produces (`for name, example in SCREENS: drive(name,
    example, ...)`) and it is what left `r1712` resolving nothing while two
    rounds published a red in it. Resolved through the SAME bindings the launch
    path uses, and against the CALLER's scope: a name means what the function
    containing the call bound it to, never what a same-named local elsewhere
    did.
    """
    consts = consts or {}
    loops = loops or {}
    enclosing = enclosing if enclosing is not None else enclosing_functions(tree)
    params: dict[str, list[str]] = {}
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = node.args
            params[node.name] = [a.arg for a in (*args.posonlyargs, *args.args)]
    out: dict[tuple[str, str], set[str]] = {}

    def literals(node: ast.expr, caller: str) -> set[str]:
        if isinstance(node, ast.Constant):
            return {node.value} if isinstance(node.value, str) else set()
        if isinstance(node, ast.Name):
            if node.id in consts:
                return {consts[node.id]}
            return set(loops.get((caller, node.id), set()))
        return set()

    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name):
            continue
        names = params.get(node.func.id)
        if names is None:
            continue
        caller = enclosing.get(id(node), "")
        for i, arg in enumerate(node.args):
            if i < len(names):
                found = literals(arg, caller)
                if found:
                    out.setdefault((node.func.id, names[i]), set()).update(found)
        for kw in node.keywords:
            if kw.arg in names and isinstance(kw.value, ast.Constant):
                if isinstance(kw.value.value, str):
                    out.setdefault((node.func.id, kw.arg), set()).add(kw.value.value)
    return out


def launched_by(path: Path, *, rosters: bool = True) -> set[str]:
    """The package names a demo script launches, by parsing it.

    Every `RpcSubprocess(...)` call in the file, wherever it appears --
    including inside a helper defined in the same file, which is why this walks
    the whole tree rather than only the top level. The first argument resolves
    from a string literal, from a module-level constant, from — R2106 — the
    literals this module passes at that parameter's position, or from — R2126 —
    a `for` target over a module-level roster.

    `rosters=False` withholds that last hop. It exists for the selftest, which
    has to be able to ask what the tool answered BEFORE R2126 to assert that the
    hop carries weight in this tree; a switch is how that question gets asked
    without a second copy of this walk to drift from it.
    """
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (SyntaxError, UnicodeDecodeError):
        return set()
    consts = module_strings(tree)
    # The module-level function each node sits inside, so a name is resolved
    # against the function that binds it.
    enclosing = enclosing_functions(tree)
    loops = (
        roster_bindings(tree, module_sequences(tree, consts), enclosing)
        if rosters
        else {}
    )
    passed = parameter_arguments(tree, consts, loops, enclosing)
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
        elif isinstance(first, ast.Name):
            owner = enclosing.get(id(node), "")
            out |= loops.get((owner, first.id), set())
            out |= passed.get((owner, first.id), set())
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


def changed_walks(paths: list[str]) -> list[Path]:
    """The demos among the changed paths — the axis a change to a WALK takes.

    ★★★★★ R2103 — the third axis, and the one the other two are blind to by
    construction. `blast_radius.py` answers *packages* and [`tracked_data`]
    answers *pins*; a change that edits nothing but Python under `tools/` has
    neither, so this tool answered **no demo** for a round that rewrote
    twenty-six walks and the shared harness. Measured on that round's own staged
    diff, `--radius` selected zero and the sweep it feeds printed *this change
    reaches no demo — nothing to run*. A radius that answers "nothing" for a
    change made entirely inside its own population is not narrow, it is absent,
    and the failure is silent in the direction that publishes unverified walks.

    ⚠ Only walks that still EXIST. `git diff --name-only` names a deleted file
    too, and a radius that selects one hands the sweep a path it cannot run.

    ⚠⚠ The neighbouring axis was measured and REJECTED. The obvious companion —
    a changed shared module under `tools/` selects every walk that imports it —
    is correct and useless here: measured at R2103 the corpus is 726 walks and
    **726 of them import `rpc_verify`**, so that axis answers the entire sweep
    for any harness edit, which is R1858's *the answer is EVERYTHING and the
    narrow set vanishes into six hundred names nobody can run*. It is reported
    as a caution in `main` instead, where a person can weigh it, rather than
    silently turning a push into a full sweep.
    """
    out = []
    for repo_path in paths:
        candidate = REPO / repo_path
        if candidate.parent == DEMOS and candidate.suffix == ".py" and candidate.exists():
            out.append(candidate)
    return sorted(set(out))


def touches_harness(paths: list[str]) -> list[str]:
    """The shared modules under `tools/` this change edits that a walk imports.

    Not an axis — a CAUTION, for the reason [`changed_walks`] records: every
    walk imports the harness, so selecting on it would mean the full sweep. What
    it is good for is saying so out loud, since a harness edit really can reach
    a walk this tool then does not select.
    """
    shared = set()
    for demo in sorted(DEMOS.glob("*.py")):
        try:
            tree = ast.parse(demo.read_text(encoding="utf-8"))
        except (SyntaxError, UnicodeDecodeError):
            continue
        for node in ast.walk(tree):
            names: list[str] = []
            if isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                names = [node.module.split(".")[0]]
            elif isinstance(node, ast.Import):
                names = [alias.name.split(".")[0] for alias in node.names]
            for name in names:
                if (REPO / "tools" / f"{name}.py").exists():
                    shared.add(f"tools/{name}.py")
    return sorted(set(paths) & shared)


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
    """Whether the file actually CALLS the launcher.

    ★★★★★ R2126 — parsed, not grepped, and this half of the tool was the last
    place the two disagreed. `launched_by` above parses precisely so that "a
    name inside a comment or a docstring is not a launch"; this asked
    `"RpcSubprocess" in text`, so the audit's population came from a rule the
    resolution half rejects.

    Measured once the roster hop had emptied the rest of the audit: the single
    remaining entry was `r1447_font_free_tui.py`, whose only occurrence of the
    name is a COMMENT saying *this demo launches outside `RpcSubprocess`*. So
    the residue the audit reported was entirely a demo telling the truth about
    itself, and a number now printed at every push would have said there is a
    blind spot where there is none — the error direction that wastes a round
    looking for something that is not there.
    """
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (SyntaxError, UnicodeDecodeError):
        return False
    return any(
        isinstance(node, ast.Call)
        and (node.func.id if isinstance(node.func, ast.Name) else getattr(node.func, "attr", None))
        == "RpcSubprocess"
        for node in ast.walk(tree)
    )


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


def audit_of(paths: list[Path]) -> list[Path]:
    """Which of `paths` call the launcher and resolve no target.

    Taken as an argument rather than read from `DEMOS` so the selftest can hand
    it fixtures: `audit()` below is the real population, and a rule tested only
    against the tree it reports on cannot be shown to discriminate at all.
    """
    return [path for path in paths if mentions_launcher(path) and not launched_by(path)]


def audit() -> list[Path]:
    """Demos that mention the launcher and whose target could not be resolved."""
    return audit_of(sorted(DEMOS.glob("*.py")))


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

    # ★★★★★ R2106 — the PARAMETER hop, against fixtures rather than against
    # this tree. A case pinned to a demo that exists rots the day that demo is
    # rewritten, which is the failure `painted_addresses` recorded for its own
    # oracle; what does not rot is the distinction being claimed.
    #
    # Three cases, and the third is the one that decides whether the hop is a
    # LOOKUP or a guess: two helpers with a parameter of the same name must not
    # pool their literals, or a demo is credited with a screen it never
    # launches. Over-selection is the direction that makes a radius useless
    # rather than merely short.
    with tempfile.TemporaryDirectory() as tmp:
        hop = Path(tmp) / "hop_demo.py"
        hop.write_text(
            "def screen(example, name):\n"
            "    with RpcSubprocess(example) as app:\n"
            "        pass\n"
            'screen("hello-node-lab", "node lab")\n'
            'screen(example="hello-packet-view", name="capture viewer")\n',
            encoding="utf-8",
        )
        if launched_by(hop) != {"hello-node-lab", "hello-packet-view"}:
            failures.append(
                f"the parameter hop resolved {sorted(launched_by(hop))!r}, not both literals"
            )
        two = Path(tmp) / "two_helpers_demo.py"
        two.write_text(
            "def drives(example):\n"
            "    with RpcSubprocess(example) as app:\n"
            "        pass\n"
            "def mentions(example):\n"
            "    return example\n"
            'drives("hello-node-lab")\n'
            'mentions("hello-packet-view")\n',
            encoding="utf-8",
        )
        if launched_by(two) != {"hello-node-lab"}:
            failures.append(
                "a literal passed to a DIFFERENT helper's parameter of the same name was "
                f"credited as a launch: {sorted(launched_by(two))!r}"
            )
        unfed = Path(tmp) / "unfed_demo.py"
        unfed.write_text(
            "def screen(example):\n    with RpcSubprocess(example) as app:\n        pass\n",
            encoding="utf-8",
        )
        if launched_by(unfed):
            failures.append(
                "a parameter nothing in the module feeds resolved to "
                f"{sorted(launched_by(unfed))!r} rather than to nothing"
            )

    # ★★★★★ R2126 — the ROSTER hop, against fixtures for R2106's reason: what
    # does not rot is the distinction being claimed, not a case pinned to a demo
    # somebody will rewrite.
    #
    # Six cases, and the second and third are the ones that decide whether this
    # is a lookup or a guess. Over-selection is the direction that makes a
    # radius useless rather than merely short, and a roster offers two fresh
    # ways into it: reading a bare target as the first COMPONENT of a tuple, and
    # pooling two functions' same-named targets.
    with tempfile.TemporaryDirectory() as tmp:
        roster = Path(tmp) / "roster_demo.py"
        roster.write_text(
            'SCREENS = [("node lab", "hello-node-lab"), ("viewer", "hello-packet-view")]\n'
            "def drive(name, example):\n"
            "    with RpcSubprocess(example) as app:\n"
            "        pass\n"
            "def main():\n"
            "    for name, example in SCREENS:\n"
            "        drive(name, example)\n",
            encoding="utf-8",
        )
        if launched_by(roster) != {"hello-node-lab", "hello-packet-view"}:
            failures.append(
                "the roster hop resolved "
                f"{sorted(launched_by(roster))!r}, not the roster's second column"
            )
        bare = Path(tmp) / "bare_target_demo.py"
        bare.write_text(
            'SCREENS = [("node lab", "hello-node-lab")]\n'
            "def main():\n"
            "    for pair in SCREENS:\n"
            "        with RpcSubprocess(pair) as app:\n"
            "            pass\n",
            encoding="utf-8",
        )
        if launched_by(bare):
            failures.append(
                "a BARE target over a roster of tuples binds the tuple, and reading it "
                f"as the first component resolved {sorted(launched_by(bare))!r}"
            )
        scoped = Path(tmp) / "scoped_target_demo.py"
        scoped.write_text(
            'LAUNCHED = ["hello-node-lab"]\n'
            'MENTIONED = ["hello-packet-view"]\n'
            "def main():\n"
            "    for example in LAUNCHED:\n"
            "        with RpcSubprocess(example) as app:\n"
            "            pass\n"
            "def other():\n"
            "    for example in MENTIONED:\n"
            "        print(example)\n",
            encoding="utf-8",
        )
        if launched_by(scoped) != {"hello-node-lab"}:
            failures.append(
                "a `for` target of the same name in a DIFFERENT function was credited as "
                f"a launch: {sorted(launched_by(scoped))!r}"
            )
        mapping = Path(tmp) / "mapping_demo.py"
        mapping.write_text(
            'BINDINGS = {"hello-node-lab": "a", "hello-packet-view": "b"}\n'
            "def main():\n"
            "    for example, expected in BINDINGS.items():\n"
            "        with RpcSubprocess(example) as app:\n"
            "            pass\n"
            "    for other in BINDINGS:\n"
            "        print(other)\n",
            encoding="utf-8",
        )
        if launched_by(mapping) != {"hello-node-lab", "hello-packet-view"}:
            failures.append(
                f"`.items()` unpacking resolved {sorted(launched_by(mapping))!r}"
            )
        values = Path(tmp) / "values_demo.py"
        values.write_text(
            'BINDINGS = {"a": "hello-node-lab"}\n'
            "def main():\n"
            "    for example in BINDINGS.values():\n"
            "        with RpcSubprocess(example) as app:\n"
            "            pass\n",
            encoding="utf-8",
        )
        if launched_by(values) != {"hello-node-lab"}:
            failures.append(f"`.values()` resolved {sorted(launched_by(values))!r}")
        unpacked_key = Path(tmp) / "unpacked_key_demo.py"
        unpacked_key.write_text(
            'BINDINGS = {"hello-node-lab": "hello-packet-view"}\n'
            "def main():\n"
            "    for example, expected in BINDINGS:\n"
            "        with RpcSubprocess(expected) as app:\n"
            "            pass\n",
            encoding="utf-8",
        )
        if launched_by(unpacked_key):
            failures.append(
                "`for a, b in <dict>` unpacks each KEY, and reading it as a row resolved "
                f"{sorted(launched_by(unpacked_key))!r}"
            )
        destructured = Path(tmp) / "destructured_demo.py"
        destructured.write_text(
            'BARE_ROOT = ("hello-node-lab", "root", "label")\n'
            "def body():\n"
            "    example, tag, slot = BARE_ROOT\n"
            "    with RpcSubprocess(example) as app:\n"
            "        pass\n",
            encoding="utf-8",
        )
        if launched_by(destructured) != {"hello-node-lab"}:
            failures.append(
                "a destructuring assignment off a roster resolved "
                f"{sorted(launched_by(destructured))!r}"
            )
        computed = Path(tmp) / "computed_roster_demo.py"
        computed.write_text(
            "SCREENS = population().walkable\n"
            "def main():\n"
            "    for example in SCREENS:\n"
            "        with RpcSubprocess(example) as app:\n"
            "            pass\n",
            encoding="utf-8",
        )
        if launched_by(computed):
            failures.append(
                "a roster whose value is computed resolved "
                f"{sorted(launched_by(computed))!r} rather than staying in the audit"
            )

    # ★★★★★ R2126 — THE VACUITY GUARD ON THE AUDIT, and it comes first because
    # the number it protects is now printed at every push.
    #
    # `audit()` is `mentions_launcher and not launched_by`, so a
    # `mentions_launcher` that answered False for everything would report ZERO
    # unresolved demos while resolving nothing — a green that means "nobody was
    # asked", which is the shape R2124 and R2125 each recorded. The zero this
    # tree currently prints is only worth printing if both halves discriminate.
    with tempfile.TemporaryDirectory() as tmp:
        calls = Path(tmp) / "calls_demo.py"
        calls.write_text(
            "def main():\n    with RpcSubprocess(whatever) as app:\n        pass\n",
            encoding="utf-8",
        )
        if not mentions_launcher(calls):
            failures.append("a real launch call was not seen as mentioning the launcher")
        if audit_of([calls]) != [calls]:
            failures.append("a demo that calls the launcher and resolves nothing left the audit")
        says = Path(tmp) / "says_demo.py"
        says.write_text(
            "# this demo launches outside RpcSubprocess, on purpose\nx = 1\n",
            encoding="utf-8",
        )
        if mentions_launcher(says):
            failures.append(
                "a demo whose only occurrence of the launcher is a COMMENT was counted "
                "as mentioning it, which is the rule `launched_by` rejects"
            )
    if not any(mentions_launcher(demo) for demo in DEMOS.glob("*.py")):
        failures.append(
            "no demo in this tree calls the launcher at all, so the audit's zero is vacuous"
        )

    # And the hop must carry weight in THIS tree, or it is a rule that reads
    # well and selects nothing. Asserted as a COMPARISON rather than as a count,
    # for the reason the substring rule above is: a count in a test goes stale
    # like a count in prose. The property is that some demo resolves a launch
    # target ONLY because a roster was read -- which is what "sat in --audit and
    # a change to its screen never selected it" was.
    only_by_roster = [
        demo
        for demo in DEMOS.glob("*.py")
        if launched_by(demo) and not launched_by(demo, rosters=False)
    ]
    if not only_by_roster:
        failures.append(
            "no demo in this tree resolves a launch target only through a roster, so "
            "the roster hop is carrying nothing"
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

    # ★★★★★ R2103 — the corpus axis, against real files rather than a story.
    #
    # The defect it repairs was a SILENCE: for a change made entirely of walks
    # this tool answered nothing, and the sweep it feeds ran nothing. So the
    # case that matters is not "does it find a walk" but "is a walk-only change
    # non-empty", and that is what is asserted.
    real_walk = sorted(DEMOS.glob("*.py"))[:1]
    if not real_walk:
        failures.append("no walk exists at all, so the corpus axis cannot be checked")
    else:
        rel = str(real_walk[0].relative_to(REPO))
        if changed_walks([rel]) != real_walk:
            failures.append(f"the corpus axis did not select {rel}, a walk this change edits")
        # A path that is not a walk must not be selected, or the axis is
        # "everything changed" wearing a narrower name.
        noise = changed_walks(
            [
                "crates/pinion-core/src/lib.rs",
                "tools/rpc_verify.py",
                "docs/phase-b-rounds.tsv",
                "tools/demos/README.md",
            ]
        )
        if noise:
            failures.append(f"the corpus axis selected non-walks: {noise}")
        # ⚠ A DELETED walk is named by `git diff --name-only` and cannot be run.
        if changed_walks(["tools/demos/r0000_deleted_by_this_change.py"]):
            failures.append("the corpus axis selected a walk that does not exist")

    # And the harness caution must name the harness and nothing else — it is the
    # axis that was measured and rejected, so it has to stay a caution.
    said = touches_harness(["tools/rpc_verify.py", "tools/demo_radius.py"])
    if said != ["tools/rpc_verify.py"]:
        failures.append(f"the harness caution named {said!r}")

    # ★★★★★ R2028 — THE ORACLE, against this repository's real index.
    #
    # Every case above hands the pure readers a fixture path list. Nothing
    # watched the function that produces the real one, and a `changed_paths`
    # answering an empty list would make this tool select NO demo for every
    # change while every case above stayed green — `tools/oracle_census.py`
    # counts that shape. `staged` because it is the mode the hooks use; the
    # answer may legitimately be empty, so this asserts its SHAPE.
    staged = changed_paths("staged", None)
    if not isinstance(staged, list) or any(not isinstance(p, str) for p in staged):
        failures.append(f"changed_paths must answer a list of paths, got {staged!r}")
    if any(p.startswith("/") or "\n" in p for p in staged):
        failures.append("changed_paths answers one repository-relative path per entry")

    # ★★★★★ R2126 — the failure lines carry the token `selftest: FAIL`, and the
    # reason is not tidiness: `tools/counterfactual.py` classifies a red gate
    # whose output matches no known marker as UNREADABLE, and R1939's rule is
    # that UNREADABLE does NOT count as a catch. So a counterfactual battery run
    # against this tool would have reported every genuine catch as a failure of
    # the round.
    #
    # Measured rather than reasoned: `classify` was handed the literal this
    # function used to print (`demo radius selftest: <sentence>`) and answered
    # UNREADABLE. The driver's `python --selftest` vocabulary is
    # `selftest: FAIL` / `selftest FAIL` / `SELFTEST FAIL`, and this tool spoke
    # none of them.
    #
    # ⚠ The token goes on the FAILURE lines only. The summary lines below print
    # on a green run too, so putting it in a shared prefix would classify every
    # passing run as a failure — the trap the driver's own comment records for
    # `painted-addresses:`.
    #
    # ⚠ Measured at the same time and NOT repaired here: 22 of this tree's 29
    # selftests are inaudible to that driver the same way. This is the one whose
    # battery this round runs; the rest are recorded in the round's entry with
    # their reproduction command, because 21 unrelated gates are not this
    # round's to move.
    for line in failures:
        print(f"demo radius selftest: FAIL — {line}", file=sys.stderr)
    print(f"demo radius selftest: {len(resolved)} demo(s) resolve a launch target")
    print(
        f"demo radius selftest: {len(named)} demo(s) name the pin the pin axis "
        "was built for"
    )
    # ★★★★★ R2126 — and the RESIDUE, printed every push rather than waiting for
    # somebody to type `--audit`. What this tool cannot resolve is the set a
    # change silently fails to select, and two rounds paid for that set being a
    # silence: `r1712` sat in it while R2123 and R2124 each published a red in
    # it. A number nobody asked for is how the build-cache budget stopped being
    # invisible, and it is the same fix.
    print(
        f"demo radius selftest: {len(audit())} demo(s) mention the launcher "
        "and resolve no target (`--audit` names them)"
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

    # ★★★★★ R2103 — the SELECTION is the union of every axis that selects, and
    # it all leaves on stdout, because stdout is what `sweep_headless.sh
    # --radius` consumes. Before this, the package axis was the selection and
    # the pin axis was a sentence on stderr a person was expected to act on —
    # so a change that edited only a pin, or only walks, selected NOTHING while
    # the tool printed a paragraph saying which demos it reached. A radius
    # printed beside the selection is not in the selection.
    why: dict[Path, set[str]] = {}
    for path, launched in hits:
        why.setdefault(path, set()).update(f"pkg:{n}" for n in launched)
    for path, hit in by_pin:
        why.setdefault(path, set()).update(f"pin:{Path(p).name}" for p in hit)
    for path in changed_walks(changed):
        why.setdefault(path, set()).add("walk:edited")

    if args.count:
        print(len(why))
        return 0
    for path in sorted(why):
        rel = path.relative_to(REPO)
        if args.command:
            print(f"python3 {rel}")
        else:
            print(f"{rel}  ({', '.join(sorted(why[path]))})")
    # ★★★★★ R1858 — and the pin axis is NAMED beside the package one rather
    # than folded into it. Folding would hide it exactly when it matters: the
    # sets are the same size only while the package answer is small, and the
    # case this exists for is the one where that answer is six hundred. R2103
    # kept the naming and moved the SELECTING half above, where a caller that
    # reads stdout gets it — the two are different jobs and only one of them
    # was being done.
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
    # ⚠ R2103 — the harness caution. Every walk imports `rpc_verify`, so an edit
    # to it can reach a walk this tool does not select, and selecting on that
    # would mean the full sweep for every harness round. Said, not decided.
    harness = touches_harness(changed)
    if harness:
        print(
            f"\n⚠ this change edits {len(harness)} shared module(s) every walk "
            f"imports ({', '.join(harness)}). Only the walks selected above are "
            f"in the radius; whether the rest are reached is a judgment about "
            f"what changed in them — the full sweep is CI's.",
            file=sys.stderr,
        )
    if pins and not by_pin:
        print(
            f"\n★ this change edits {len(pins)} tracked data file(s) and NO demo "
            f"names any of them: {', '.join(pins)}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
