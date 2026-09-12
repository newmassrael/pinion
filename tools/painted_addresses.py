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

# ★★★★★ TWO POPULATIONS, ONE RATCHET (R2116)

Until R2116 the population here was Python alone — the walks and the harness —
and this docstring carried its own limit as a warning: *the RUST readers are not
in it, so this tool's number is the WALKS' remainder, not the tree's; asking it
"is the address debt repaid" gets an answer about a third of the tree.*

That warning stood for sixty-two rounds while seventeen instalments were paid
against it, and the reason it could not simply be lifted is the reason every
gate in this campaign arrived late: **the Rust side is not at zero and will not
be for many rounds, and a gate that is red on the day it is written is a gate
nobody turns on.** A ratchet has no such problem — it pins what stands and
refuses a rise — which is what this file already was for the walks.

So each family now carries TWO numbers, ratcheted independently:

* `walk` — sites in `tools/demos/` and the modules those walks import.
* `rust` — sites in `examples/*/src/`, excluding each screen's `address.rs`.

⚠ **`crates/` is deliberately NOT in the Rust population, and that is measured
rather than assumed.** R2105 counted 129 Rust spellings of one screen's family
and found 43 of them in `pinion-core` and `pinion-rpc`, where `ShrinkPolicy`
uses that screen's addresses as ARBITRARY EXAMPLE TAGS — 42 inside `#[cfg(test)]`
and one in a doc comment. Those are not readers of a screen and can never be
converted, so counting them would pin a floor no round could ever lower, and the
one number this gate exists to make monotone would stop meaning anything. The
framework's own compositions have declaring sites inside their crates
(`config_form::address`, R2050/R2052) and gates of their own.

⚠⚠ **The two columns draw the comment line differently, on purpose.** The walk
census reads structure, so a comment is not in the tree at all; the Rust census
reads string literals, so a comment is not one either. What the Rust column
therefore CANNOT see is a screen module's prose spelling an address — and that
is held instead by each screen's own gate, whose needle is the bare stem and
which refuses a comment like any other speller (`r2116_a_reset_seat_address_…`).
Three gates, three populations, and this file now says which is which instead of
leaving a reader to find out.

Usage:
    python3 tools/painted_addresses.py --check         # the ratchet (pre-push)
    python3 tools/painted_addresses.py --selftest      # its own tests
    python3 tools/painted_addresses.py --owed          # the work order
    python3 tools/painted_addresses.py --list <stem>   # every site in a family
    python3 tools/painted_addresses.py --unspelled     # which families Rust never spells
    python3 tools/painted_addresses.py --write-budget            # re-pin
    python3 tools/painted_addresses.py --write-budget --pin lab.form
                                       # ...and record a family as converted
"""

from __future__ import annotations

import argparse
import ast
import functools
import json
import re
import sys
from pathlib import Path
from typing import Iterable, NamedTuple

ROOT = Path(__file__).resolve().parent.parent
BUDGET = ROOT / "docs" / "painted-address-budget.tsv"

#: The families that were converted and whose value nothing pins — see
#: [`deleted_checks`]. Tracked, so the list only shrinks by a reviewable diff.
UNPINNED = ROOT / "docs" / "unpinned-families.tsv"

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

#: The trees a painted address is COMPOSED in — the Rust side of the wall.
#:
#: [`sources`] is the corpus that SPELLS addresses (python); this is the corpus
#: that MAKES them. Two different questions, so two names.
#:
#: ⚠ Used by [`unspelled`], which asks *does Rust spell this stem ANYWHERE* and
#: wants the widest possible corpus for that. The RATCHET's Rust population is
#: narrower — see [`RUST_CENSUS_ROOT`] and the module header for why.
RUST_ROOTS: tuple[str, ...] = ("crates", "examples")

#: The tree the Rust half of the ratchet counts, and the file it excludes.
#:
#: ★★★★★ R2116 — the screens and the hosts that mount them, which is where this
#: campaign's Rust remainder lives. `address.rs` is each screen's DECLARING site:
#: it is supposed to be full of these literals, and counting it would charge a
#: family for being declared.
#:
#: ⚠ The exclusion is by FILE NAME rather than by a list of paths, so a screen
#: that gains a declaring module joins the arrangement the day it does. What
#: keeps that honest is [`selftest`], which asserts the census sees more than one
#: crate and that at least one `address.rs` exists to be excluded — a filter that
#: matched nothing would be silently equivalent to no filter at all.
#:
#: ★★★★★ R2155 — **a SET, because a screen names more than one dotted
#: vocabulary.** `key.rs` declares the configuration keys the node lab's form
#: edits: `transport.link.tx.batch_size` has exactly the shape this file's
#: needle reads, and is not an address at all. Its declaration is a declaring
#: site on the same terms `address.rs` is, and charging it would mean that
#: vocabulary can never reach zero however completely it is converted.
#:
#: ⚠ A NAME cannot be added here on its own authority. What makes an entry
#: true is a gate inside the crate holding that file to be the only speller of
#: its vocabulary — `r2049_a_role_address_is_typed_in_one_place` for the first,
#: `r2155_a_config_key_is_typed_in_one_place` for the second. Without such a
#: gate a name here is a blanket exemption for whatever the file contains,
#: which is the shape this whole campaign exists to remove. It is a list of two
#: rather than a derivation because the property is *designation*, not syntax:
#: `const DIALLED_KEY: &str = "connect.endpoints"` was a `pub const` in a
#: declaring-looking shape and was a genuine second spelling, so a rule reading
#: the site rather than the file would have excused it.
RUST_CENSUS_ROOT = "examples"
RUST_DECLARING_FILES = ("address.rs", "key.rs")
#: The first of [`RUST_DECLARING_FILES`], kept as its own name because the
#: selftest asserts one exists to be excluded and the message names it.
RUST_DECLARING_FILE = RUST_DECLARING_FILES[0]

#: A Rust string literal, escapes handled, quotes included.
#:
#: ⚠ Approximate, and the direction of the approximation is the point: a raw
#: string (`r#"..."#`) reads here as an ordinary literal, and a commented line
#: carrying a lone quote can open one. Both errors can only make a stem look
#: MORE spelled, never less, so a family this reports as unspelled is a claim
#: that errs towards under-claiming silence.
RUST_LITERAL = re.compile(r'"[^"\\]*(?:\\.[^"\\]*)*"', re.DOTALL)


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


def rust_sites_in(text: str) -> list[tuple[int, str]]:
    """`(line, family stem)` for every spelled painted address in Rust `text`.

    ★★★★★ R2116 — PURE, and handed its text rather than a path, for the reason
    [`rust_needles`] is: the discrimination this makes is what has to be tested,
    and a case pinned to today's `examples/` rots the moment the family it names
    is converted. The reader that hands it the world is asserted separately.

    A literal is a site when the address is ANCHORED at its start — the same rule
    the Python half uses, and for the same reason. A Rust doc comment is not a
    literal, so prose is out of this population; what covers prose is each
    screen's own gate, whose needle is the bare stem.

    ⚠ [`RUST_LITERAL`] is approximate in one direction only (a raw string reads
    as an ordinary one; a comment carrying a lone quote can open one), so this
    can over-count and cannot under-count. Against a ratchet an over-count is a
    stable offset rather than a hole: it makes the pinned number larger than the
    truth, and a family whose real count RISES still rises here.
    """
    found: list[tuple[int, str]] = []
    for match in RUST_LITERAL.finditer(text):
        hit = ADDRESS.match(match.group(0)[1:])
        if hit:
            found.append((text.count("\n", 0, match.start()) + 1, hit.group(1)))
    return sorted(found)


def test_spans(text: str) -> list[tuple[int, int]]:
    """The 1-based line spans of Rust `text` that a `#[cfg(test)]` item covers.

    ★★★★★ R2144 — **an assertion and a reader are not the same site, and this
    is what tells them apart.**

    A production reader that spells an address is the defect: a wrong letter
    compiles, paints, and every query looking for the mark answers nothing.
    An ASSERTION that spells one is the opposite — a wrong letter fails loudly,
    and while a family's tests still spell its literals they are the only thing
    pinning that family's address VALUES (R2137.3 measured the converse: the
    five fully converted screens have zero assertions and a consistent rename
    passed 790 tests). So converting an assertion does not repay this debt; it
    removes a check and creates the other one.

    ⚠⚠ **The obvious rule is wrong, and it fails in the dangerous direction.**
    The first draft of this classifier took every site at or after the FIRST
    `#[cfg(test)]` line to be an assertion. Measured on this tree, **54 files
    declare a top-level item after that marker** — `const WIN_W`, a helper
    `fn`, a `pub const` fixture — so the rule misread production readers as
    assertions and UNDERSTATED the reader debt by 60 sites, 42%. An
    under-count is the direction that hides work, which is why this brace-
    matches the block instead. Do not "simplify" it back.

    ⚠ Approximate in one direction only, like [`RUST_LITERAL`]: a brace inside
    a string or a comment inside a `#[cfg(test)]` item can close the span
    early, which ends it too soon and counts the tail as PRODUCTION. That
    over-states the reader debt rather than hiding it.
    """
    lines = text.splitlines()
    spans: list[tuple[int, int]] = []
    for start, line in enumerate(lines):
        if not re.match(r"\s*#\[cfg\(test\)\]", line):
            continue
        depth, opened, cursor = 0, False, start
        while cursor < len(lines):
            for char in lines[cursor]:
                if char == "{":
                    depth += 1
                    opened = True
                elif char == "}":
                    depth -= 1
            if opened and depth <= 0:
                spans.append((start + 1, cursor + 1))
                break
            cursor += 1
    return spans


def rust_roles(path: Path, text: str) -> tuple[int, int]:
    """`(reader, assertion)` counts for the spelled sites in one Rust file.

    A comment line is neither: it is prose about an address, which
    [`rust_sites_in`] already declines to treat as a literal.
    """
    spans = test_spans(text)
    lines = text.splitlines()
    reader = assertion = 0
    for line, _stem in rust_sites_in(text):
        if lines[line - 1].lstrip().startswith("//"):
            continue
        if any(first <= line <= last for first, last in spans):
            assertion += 1
        else:
            reader += 1
    return reader, assertion


def rust_role_totals() -> tuple[int, int]:
    """`(reader, assertion)` over the whole Rust population."""
    reader = assertion = 0
    for path in rust_sources():
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        one, two = rust_roles(path, body)
        reader += one
        assertion += two
    return reader, assertion


@functools.lru_cache(maxsize=1)
def rust_sources() -> tuple[Path, ...]:
    """Every Rust file in the ratchet's Rust population.

    Derived from the tree, never listed — R2053's lesson, which this file
    already applies to its Python half.

    ⚠ A crate's LIBRARY sources, so `build.rs` is out. That is a statement about
    what a reader of a painted address is rather than a filter on what an
    address looks like: a build script runs before there is a scene and cannot
    read a mark. Measured when this population was first counted — every one of
    the 224 build scripts names `app.pinion.xml`, the forge input, and the
    address needle matched all 224 as a family. R2103 refused to put a
    heuristic in the NEEDLE for exactly this shape and was right; the answer is
    to say correctly which files are readers.
    """
    return tuple(
        path
        for path in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/src/**/*.rs"))
        if path.name not in RUST_DECLARING_FILES
    )


def rust_scan() -> dict[str, list[tuple[str, int]]]:
    """Every spelled Rust site in the population, indexed by family stem."""
    index: dict[str, list[tuple[str, int]]] = {}
    for path in rust_sources():
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        name = str(path.relative_to(ROOT))
        for line, stem in rust_sites_in(body):
            index.setdefault(stem, []).append((name, line))
    return index


def rust_census(
    index: dict[str, list[tuple[str, int]]] | None = None,
) -> dict[str, int]:
    """How many Rust sites spell an address under each family stem."""
    index = rust_scan() if index is None else index
    return {stem: len(found) for stem, found in index.items()}


class Pinned(NamedTuple):
    """A family's two budgeted numbers.

    ★ A pair rather than two dicts, so a caller cannot read one column and
    believe it has the family's remainder — the defect this whole extension is
    about, one level up.
    """

    walk: int
    rust: int


def read_budget() -> dict[str, Pinned]:
    """The pinned pair per family.

    ⚠ A row with one field is read as walk-only with the Rust column unknown,
    and unknown is recorded as zero — which makes the FIRST run after this
    format change report every Rust count as a rise. That is the correct
    behaviour and the round that changed the format re-pinned deliberately; a
    reader that silently accepted the old shape as "no claim" would have let the
    whole Rust population in unmeasured.
    """
    if not BUDGET.exists():
        return {}
    out: dict[str, Pinned] = {}
    for line in BUDGET.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        stem = parts[0]
        walk = int(parts[1]) if len(parts) > 1 and parts[1] else 0
        rust = int(parts[2]) if len(parts) > 2 and parts[2] else 0
        out[stem] = Pinned(walk, rust)
    return out


def write_budget(
    counts: dict[str, int],
    rust_counts: dict[str, int],
    pin: str | None = None,
) -> None:
    """Pin what stands in both populations, and keep a converted family at zero.

    ★ A family the scan no longer finds is carried forward at 0 rather than
    dropped. A dropped row would let the next round re-acquire a speller in a
    family a previous one paid off, and the gate would call that a NEW family
    only if the stem were new — which it is not.

    ★★ `pin` is how a family FIRST reaches zero. A converted family leaves no
    trace in either corpus to be found by scanning — that is what converting it
    means — so the round that pays one off says so, and the claim is checked at
    the moment it is made rather than taken on trust.

    ★★★ R2116 — a pin now requires BOTH columns at zero. A family converted in
    the walks and still spelled in five Rust modules is not converted; the
    single-column pin said it was, and eleven families carry that claim today.
    """
    stems = set(counts) | set(rust_counts) | set(read_budget())
    kept = {
        stem: Pinned(counts.get(stem, 0), rust_counts.get(stem, 0)) for stem in stems
    }
    if pin is not None:
        standing = kept.get(pin, Pinned(0, 0))
        if standing.walk or standing.rust:
            raise SystemExit(
                f"painted-addresses: refusing to pin {pin} at zero — it is still "
                f"spelled {standing.walk} time(s) in the walks and "
                f"{standing.rust} time(s) in Rust. Ask `--list {pin}` for where."
            )
        kept[pin] = Pinned(0, 0)
    lines = [
        "# R2054 — how many sites SPELL a painted address instead of asking the",
        "# screen for it, by family stem. R2116 — in TWO populations.",
        "#",
        "#   column 2 `walk` — sites in tools/demos/ and the modules they import",
        "#   column 3 `rust` — sites in examples/*/src/, minus each screen's",
        "#                     own address.rs, which is where they are declared",
        "#",
        "# The gate allows a count to FALL or hold, refuses a rise, and refuses a",
        "# family stem it has never seen. A family at 0 is PINNED there: it was",
        "# converted, and a speller reappearing in it is a regression.",
        "#",
        "# The fix at a site is to read the address off the wire — a screen",
        "# publishes its roles' rows, its form rows' controls, the prefix each",
        "# part of a form row is addressed under, and its rail's seats. In Rust",
        "# the fix is to name the const the screen's `address.rs` declares.",
        "#",
        "# Rewritten by `tools/painted_addresses.py --write-budget`; do not",
        "# hand-edit.",
    ]
    lines += [f"{stem}\t{n.walk}\t{n.rust}" for stem, n in sorted(kept.items())]
    BUDGET.write_text("\n".join(lines) + "\n", encoding="utf-8")
    # ★ R2147 — the second ratchet is re-pinned by the SAME command, because a
    # round that re-pins one and forgets the other leaves the pair disagreeing
    # about the tree they both describe.
    write_unpinned(deleted_checks(counts, rust_counts, kept))


def _risen(
    now: dict[str, int], before: dict[str, Pinned], column: str
) -> list[tuple[str, int, int]]:
    """Families whose count in `column` is above what the budget pins."""
    return sorted(
        (stem, getattr(before[stem], column) if stem in before else 0, n)
        for stem, n in now.items()
        if n > (getattr(before[stem], column) if stem in before else 0)
    )


def check() -> int:
    index, rust_index = scan(), rust_scan()
    now, rust_now = census(index), rust_census(rust_index)
    before = read_budget()
    if not before:
        print(
            "painted-addresses: no budget yet — run --write-budget once to pin "
            "what stands",
            file=sys.stderr,
        )
        return 1
    bad = False
    for column, values, found, what in (
        ("walk", now, index, "a walk"),
        ("rust", rust_now, rust_index, "a Rust reader"),
    ):
        risen = _risen(values, before, column)
        if not risen:
            continue
        bad = True
        print(
            f"painted-addresses: {what} spells a painted address it could ask for",
            file=sys.stderr,
        )
        for stem, was, is_now in risen:
            known = "" if stem in before else " (a family this gate has not seen)"
            print(f"  [{column}] {stem}: {was} -> {is_now}{known}", file=sys.stderr)
            for path, line in found.get(stem, []):
                print(f"      {path}:{line}", file=sys.stderr)
    if bad:
        print(
            "painted-addresses: a spelled address is a second copy of a\n"
            "            composition the screen already publishes, and a wrong\n"
            "            letter in it reads as the screen not painting the mark.\n"
            "            In a walk, ask the screen — see\n"
            "            `rpc_verify.form_part_prefixes` and\n"
            "            `rpc_verify.address_prefix`. In Rust, name the const\n"
            "            the screen's own `address.rs` declares. If the family\n"
            "            really has nothing to derive from, say so and re-run\n"
            "            `python3 tools/painted_addresses.py --write-budget`.",
            file=sys.stderr,
        )
        return 1
    families = set(now) | set(rust_now) | set(before)
    walk_total, rust_total = sum(now.values()), sum(rust_now.values())
    budget_total = sum(n.walk + n.rust for n in before.values())
    pinned = sum(1 for n in before.values() if n.walk == 0 and n.rust == 0)
    gone = budget_total - walk_total - rust_total
    trend = f", {gone} fewer than the budget" if gone > 0 else ""
    print(
        f"painted-addresses: {walk_total} walk + {rust_total} rust spelled "
        f"site(s) in {len(families)} family/ies{trend}; {pinned} family/ies "
        f"pinned at zero in both"
    )
    # ★★★★★ R2144 — and WHAT THOSE SITES ARE, which the totals above cannot
    # say. The ratchet counts a spelled address the same whether a painter
    # composed it or a test asserted it, and the two want opposite treatment:
    # a reader is the debt, an assertion is the only value pin a family that
    # still has one has got. Printed every run because the number this debt is
    # judged by has a FLOOR it must not cross, and a floor nobody states is a
    # target nobody can reach.
    reader, assertion = rust_role_totals()
    print(
        f"painted-addresses: of the rust half, {reader} reader(s) and "
        f"{assertion} assertion(s) — the reducible queue is "
        f"{walk_total + reader} (walks are all readers), and the census cannot "
        f"legitimately fall below {assertion}"
    )
    # ★★★★★ R2147 — and WHETHER EACH FAMILY'S VALUE IS HELD AT ALL, which
    # neither line above can say. Converting a family's last speller removes
    # the only comparison its address had unless an artifact or an assertion
    # pins it, so the campaign's own progress can DELETE checks — and until
    # this line nothing counted them.
    pins = pin_sources(families)
    by_artifact = sum(1 for source in pins.values() if source == "artifact")
    by_assertion = sum(1 for source in pins.values() if source == "assertion")
    gone_checks = deleted_checks(now, rust_now, families)
    print(
        f"painted-addresses: {by_artifact} family/ies pinned by an artifact, "
        f"{by_assertion} by an assertion, "
        f"{len(families) - by_artifact - by_assertion} by NOTHING — of those, "
        f"{len(gone_checks)} are already converted, which is a check deleted "
        f"rather than a debt repaid"
    )
    # ★★★★★ R2156 — the SECOND vocabulary, whose walk half had no gate at all.
    # Printed every run, zero or not: the number whose absence let this go
    # unseen is the one this line carries.
    keys = spelled_config_keys()
    print(
        f"painted-addresses: {len(keys)} walk site(s) spell a sourced "
        f"configuration key (a walk asks the screen which row plays the role "
        f"it means; the answer here is zero, not a budget)"
    )
    if keys:
        bad = True
        print(
            "painted-addresses: a walk spells a configuration key it could ask "
            "for",
            file=sys.stderr,
        )
        for path, line, key in keys:
            print(f"  [key] {path}:{line}: {key}", file=sys.stderr)
        print(
            "painted-addresses: a configuration key is the TARGET's word, not\n"
            "            the screen's, and a walk that spells one is claiming\n"
            "            which row it means without anything checking that. Ask\n"
            "            the screen instead — `rpc_verify.form_row(tf, <role>)`\n"
            "            hands over the row playing a role, and the screen is\n"
            "            what knows which key that is. If the role you need is\n"
            "            not published, add it to that screen's roster; do not\n"
            "            spell the key.",
            file=sys.stderr,
        )
        return 1
    joined = sorted(set(gone_checks) - read_unpinned())
    if joined:
        print(
            "painted-addresses: family/ies reached zero spellers with nothing "
            "pinning their value",
            file=sys.stderr,
        )
        for stem in joined:
            print(f"  [pin] {stem}: converted, and no artifact or assertion "
                  f"holds its address to a value", file=sys.stderr)
        print(
            "painted-addresses: the literal a reader used to carry was compared\n"
            "            with the paint on every run; nothing is now. Pin the\n"
            "            family FIRST — a screen's address-pin test, or a crate\n"
            "            that emits its grammar — then convert. If the family is\n"
            "            genuinely pinned by something this does not know about,\n"
            "            teach `pin_sources` and re-run --write-budget.",
            file=sys.stderr,
        )
        return 1
    return 0


def owed() -> int:
    """The work order: the families with the most sites left to convert.

    ★★★★★ R2116 — BOTH columns, and the sort is on their sum. Until this round
    the answer was the walks' remainder alone, and three consecutive rounds
    opened by reconciling that number with a hand count of the Rust side that
    the tool could not produce. A work order that names a third of the work is
    not a work order.
    """
    index, rust_index = scan(), rust_scan()
    stems = sorted(
        set(index) | set(rust_index),
        key=lambda s: (-(len(index.get(s, [])) + len(rust_index.get(s, []))), s),
    )
    if not stems:
        print("painted-addresses: nothing spelled anywhere")
        return 0
    print(f"{'walk':>6}  {'rust':>6}  {'files':>5}  family")
    for stem in stems:
        walk, rust = index.get(stem, []), rust_index.get(stem, [])
        files = len({path for path, _ in walk} | {path for path, _ in rust})
        print(f"{len(walk):>6}  {len(rust):>6}  {files:>5}  {stem}")
    walk_total = sum(len(f) for f in index.values())
    rust_total = sum(len(f) for f in rust_index.values())
    print(
        f"{walk_total:>6}  {rust_total:>6}         total across "
        f"{len(stems)} family/ies"
    )
    return 0


def rust_needles(text: str, stem: str) -> tuple[bool, bool]:
    """Does this Rust source spell `stem` — anywhere, and inside a literal?

    ★★★★★ R2103 — two needles rather than one, because their DISAGREEMENT is
    the fact worth having, and folding them is what let a paragraph of prose be
    wrong. R2055 prescribed separating this census's true families from its
    noise by asking whether the stem appears in the Rust tree. Whether such a
    filter keeps or deletes a given family can come down to a single doc
    comment: measured at R2103, `expanded.struct` — a QUERY PATH a walk asks a
    screen for, not a mark anything paints — is spelled in Rust exactly once,
    in a comment. A reader that answered only *yes* could not say so, and the
    hand-written note recording that refutation put this family among the ones
    Rust never spells, which it is not.

    Pure, and handed its text rather than reading the tree, so its cases are
    fixtures: the whole reason the prose went wrong is that a count measured
    once against a moving corpus was written down as though it would hold.
    """
    if stem not in text:
        return (False, False)
    return (True, any(stem in lit for lit in RUST_LITERAL.findall(text)))


@functools.lru_cache(maxsize=1)
def rust_text() -> str:
    """Every Rust source under [`RUST_ROOTS`], joined into one string.

    Joined rather than kept per file because every caller asks *does anything
    at all spell this*, which one pass answers for a hundred families. Held for
    the life of the process: reading it is real IO and [`unspelled`] asks once
    per family. A file that cannot be read is skipped rather than fatal — this
    is a diagnostic, and most of an answer beats none.
    """
    parts: list[str] = []
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                parts.append(path.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
    return "\n".join(parts)


#: Where a family's VALUE is held, so that renaming the address moves it.
#:
#: ★★★★★ R2147 — **this campaign's own progress deletes checks, and nothing was
#: counting them.** A family's literals are what holds its address to a VALUE:
#: R2137.3 renamed one consistently and 790 tests, four censuses and a walk all
#: passed, so the declaration improves the LIKELIHOOD of a typo and creates no
#: DETECTION. Converting a family's last speller therefore removes the only
#: check it had — unless something else pins the value.
#:
#: Two things do, and both are derived from the tree rather than listed:
#:
#: * a screen's `.pin` artifact, regenerated by its own test and compared byte
#:   for byte (R2139), and
#: * an ASSERTION — a literal inside a `#[cfg(test)]` span, which fails loudly
#:   when the address moves. R2146's counterfactual measured one doing exactly
#:   that in a crate.
#:
#: ⚠ The assertion corpus here is [`RUST_ROOTS`], which is WIDER than the
#: ratchet's [`RUST_CENSUS_ROOT`]. That is deliberate and it is a different
#: question: the ratchet asks *how much is left to convert* in the tree this
#: campaign is repaying, and this asks *is the value held anywhere at all*. A
#: crate's own test pins a family the ratchet does not count.
PIN_ARTIFACTS = "examples/*/src/*.pin"

#: ★★★★★ R2155 — the artifact that pins a family which is **not an address at
#: all**, and whose absence here would charge a genuinely repaid vocabulary as
#: a deleted check.
#:
#: A screen's dotted names come from two vocabularies of identical shape: the
#: addresses of painted marks, and the CONFIGURATION keys of the document it
#: edits (`transport.link.tx.batch_size`). The needle above reads shape, so it
#: counted both. R2155 gave the second one its declaring module, and the
#: families then reached zero spellers — correctly, because the keys stopped
#: being re-typed.
#:
#: What holds their VALUE is this file: the target's own declared option
#: surface, compiled into the screen by `include_str!`, and asserted key by key
#: by `r2155_a_config_key_is_sourced`. That is a value pin of the same kind as
#: a `.pin` or an emitted grammar — a committed artifact, compared against the
#: declaration by a test — so it belongs beside them rather than as a family
#: this tool reports nothing holds.
#:
#: ⚠ Read for its `paths` list only. A path here is a leaf of a configuration
#: document, never a mark on a screen, and nothing else in this tool should
#: treat the two as one: this is the one question — *is this family's value
#: held by something committed?* — where they have the same answer.
CONFIG_SURFACE_ARTIFACTS = "docs/analyzer-config-surface.json"

#: ★★★★★ R2153 — the OTHER artifact that pins a value, and the one whose
#: absence here charged a repaid family as a deleted check.
#:
#: A crate that composes addresses emits its grammar as a committed artifact,
#: regenerated from the same literal the composers are declared with and
#: compared byte for byte by that crate's own test (R2146). That is a value pin
#: of the same strength as a screen's `.pin`, with one difference that matters
#: here: its rows are TEMPLATES, `{prefix}.area.{index}`, so the family it pins
#: is `area` under **every** prefix rather than under one.
#:
#: ⚠ Before R2153 this was invisible to [`pin_sources`], and the effect was
#: precisely backwards. `chart.area` read as pinned — by the crate's own tests,
#: which use the default prefix — while `line.area`, the SAME grammar in the
#: same artifact, read as pinned by nothing. So converting a chart on a custom
#: prefix was reported as deleting a check, and the report was wrong.
#:
#: ⚠⚠ Widening a pin test is the dangerous direction (see [`covers`]), so the
#: claim is stated rather than assumed: what the artifact holds is the GRAMMAR,
#: which is what a converted reader now composes with. What it does not hold is
#: which prefix a given chart took — and that is published on the frame since
#: R2153 and pinned by the reading walk's own assertion. Two checked halves,
#: where before there was one literal.
GRAMMAR_ARTIFACTS = "crates/*/src/painted_grammar.tsv"


@functools.lru_cache(maxsize=1)
def pin_artifact_addresses() -> tuple[str, ...]:
    """Every address the committed `.pin` artifacts hold, fold markers removed.

    `repeating_site` folds an index into `#*` so a pin is stable against fixture
    data; the marker is dropped here because this asks which FAMILY is covered,
    and a family is the same one whichever row of it was folded.
    """
    out: list[str] = []
    for path in sorted(ROOT.glob(PIN_ARTIFACTS)):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for line in text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                out.append(line.replace("#*", ""))
    out.extend(config_surface_paths())
    return tuple(out)


@functools.lru_cache(maxsize=1)
def config_surface_paths() -> tuple[str, ...]:
    """Every leaf of the sourced configuration surface — see
    [`CONFIG_SURFACE_ARTIFACTS`] for why a config key is answered by this
    question and by no other one in this file.

    ⚠ A missing or unreadable artifact answers EMPTY rather than raising, which
    is the same choice the `.pin` reader above makes: this reader decides
    whether a value is held, and a reader that crashed would stop the census
    saying anything about the other 160 families over one file.
    """
    path = ROOT / CONFIG_SURFACE_ARTIFACTS
    try:
        doc = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return ()
    rows = doc.get("paths") or []
    return tuple(
        row["path"] for row in rows if isinstance(row, dict) and row.get("path")
    )


@functools.lru_cache(maxsize=1)
def grammar_parts() -> tuple[str, ...]:
    """Every family a committed grammar artifact pins, as its fixed segments.

    A template is `{prefix}.<fixed>.<fixed>.{arg}…`; what it pins is the fixed
    run between the prefix and the first argument — `area`, `focus.series`,
    `grid.minor.x`. A template with no fixed segment after the prefix pins no
    family and is skipped rather than folded into one.
    """
    out: list[str] = []
    for path in sorted(ROOT.glob(GRAMMAR_ARTIFACTS)):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for line in text.splitlines():
            line = line.strip()
            if not line or line.startswith("#") or line.count("\t") < 2:
                continue
            _kind, _name, value = line.split("\t", 2)
            head, marker, rest = value.partition("{prefix}")
            if head or not marker or not rest.startswith("."):
                continue
            fixed: list[str] = []
            for segment in rest[1:].split("."):
                if segment.startswith("{"):
                    break
                fixed.append(segment)
            if fixed:
                out.append(".".join(fixed))
    return tuple(sorted(set(out)))


#: How a chart is given a prefix, and how a `const` holding one is written.
GRAMMAR_PREFIX_CALL = re.compile(r"\.with_tag_prefix\(\s*([A-Za-z_][A-Za-z0-9_]*|\"[^\"]*\")")
GRAMMAR_PREFIX_CONST = re.compile(
    r"const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*\"([^\"]*)\"", re.MULTILINE
)


@functools.lru_cache(maxsize=1)
def grammar_prefixes() -> tuple[str, ...]:
    """Every prefix a chart in this tree is actually given.

    ⚠⚠ This exists because [`grammar_parts`] alone is the UNSAFE direction.
    The parts a chart grammar declares are ordinary words — `bar`, `label`,
    `point`, `axis`, `rule`, `slice` — so a screen family called `panel.label`
    would be claimed as pinned by an artifact that has never heard of it, and
    the whole reason [`covers`] is anchored on both sides is that calling a
    family pinned when it is not loses a check silently. Measured at R2153 no
    family in this tree collides today; every one of these words invites one.

    So a grammar pin needs BOTH halves: the part must be a declared grammar and
    the prefix must be one a chart was actually constructed with. Both are read
    from the source rather than listed — a chart given a new prefix joins the
    day it is written.

    A prefix passed as a `const` is resolved from that file's own consts, which
    is how four of this tree's charts spell it. One that cannot be resolved
    (built at run time, or named in another module) is simply not in the set,
    and the family reads as unpinned — the safe direction, and the one that
    asks a person to look.
    """
    found: set[str] = set()
    default = None
    for path in sorted(ROOT.glob(GRAMMAR_ARTIFACTS)):
        try:
            for line in path.read_text(encoding="utf-8").splitlines():
                if line.startswith("const\tDEFAULT_PREFIX\t"):
                    default = line.split("\t", 2)[2].strip()
        except OSError:
            continue
    if default:
        found.add(default)
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if "with_tag_prefix" not in text:
                continue
            consts = dict(GRAMMAR_PREFIX_CONST.findall(text))
            for raw in GRAMMAR_PREFIX_CALL.findall(text):
                if raw.startswith('"'):
                    found.add(raw.strip('"'))
                elif raw in consts:
                    found.add(consts[raw])
    return tuple(sorted(found))


def grammar_pins(stem: str) -> bool:
    """Whether a committed grammar artifact holds `stem`'s family.

    The stem is `<prefix>.<part>`. Both halves must answer: the part must be a
    family some crate declares a grammar for, and the prefix must be one a
    chart in this tree is actually given — see [`grammar_prefixes`] for why the
    second half is not optional.
    """
    prefix, _, part = stem.partition(".")
    return bool(part) and part in grammar_parts() and prefix in grammar_prefixes()


def covers(address: str, stem: str) -> bool:
    """Whether `address` holds `stem`'s segments, contiguously.

    ⚠ Containment rather than a prefix test, because a screen's pin holds WHOLE
    addresses and a family stem can sit inside one: `card.alarms.<i>.feed.head`
    pins the `feed.head` family as surely as it pins `card.alarms`. Renaming
    either segment moves that pin line.

    ⚠⚠ And the boundaries are load-bearing in the SAFE direction. Calling a
    family pinned when it is not would let a round convert its last speller and
    lose the check silently — the defect this whole derivation exists to
    surface — so the match is anchored at separators on both sides and
    `feed.header` does not answer for `feed.head`.
    """
    return f".{stem}." in f".{address}."


@functools.lru_cache(maxsize=1)
def asserted_families() -> dict[str, int]:
    """family stem -> how many ASSERTIONS spell it, over the wide Rust corpus."""
    found: dict[str, int] = {}
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if "#[cfg(test)]" not in text:
                continue
            spans = test_spans(text)
            lines = text.splitlines()
            for line, stem in rust_sites_in(text):
                if lines[line - 1].lstrip().startswith("//"):
                    continue
                if any(first <= line <= last for first, last in spans):
                    found[stem] = found.get(stem, 0) + 1
    return found


def pin_sources(families: Iterable[str]) -> dict[str, str]:
    """family -> what holds its value: `artifact`, `assertion`, or `` (nothing).

    The artifact wins when both hold, because it is the stronger claim: it is
    compared byte for byte and it covers the family whether or not any test
    happens to mention it.
    """
    addresses = pin_artifact_addresses()
    asserted = asserted_families()
    out: dict[str, str] = {}
    for stem in families:
        if any(covers(address, stem) for address in addresses) or grammar_pins(stem):
            out[stem] = "artifact"
        elif asserted.get(stem):
            out[stem] = "assertion"
        else:
            out[stem] = ""
    return out


def deleted_checks(
    now: dict[str, int], rust_now: dict[str, int], families: Iterable[str]
) -> list[str]:
    """The families that are CONVERTED and pinned by nothing.

    ★★★★★ A family here is not work that is left — it is a check that is gone.
    Every reader stopped spelling its address, which is the repair, and nothing
    holds the address to a value any more, which is worse than the state before
    the repair: the literal a reader used to carry was at least compared with
    the paint on every run.
    """
    pins = pin_sources(families)
    return sorted(
        stem
        for stem in families
        if not now.get(stem, 0) and not rust_now.get(stem, 0) and not pins[stem]
    )


def spelled_config_keys() -> list[tuple[str, int, str]]:
    """`(file, line, key)` for every WALK site spelling a sourced config key.

    ★★★★★ R2156 — the second vocabulary's walk-side gate, and the half that had
    none. R2155 gave the configuration keys a declaring module and a Rust gate;
    a walk cannot call that module, so its answer is to ask the screen which row
    plays the role it means ([`rpc_verify.form_row`]). Nothing was watching
    whether it did.

    ⚠ **This is NOT the address ratchet and does not budget.** A spelled address
    is sometimes a legitimate value pin — that is why the census above pins
    rather than demands zero. A spelled config KEY never is: the value is pinned
    by the surface artifact and by `r2155_a_config_key_is_sourced`, so a walk
    carrying one is a second, unchecked claim about which row it means and
    nothing else. The answer is zero, it IS zero, and a gate that can say so is
    worth more than a budget that lets it drift.

    ⚠ **Dotted paths only, and that is measured.** Seven sourced paths are
    single words — `id`, `mode`, `namespace`, `metadata`, `plugins`,
    `downsampling`, `low_pass_filter`. Measured at R2156, eighteen unrelated
    examples in this tree spell one of those for their own reasons, so a needle
    over them would report a defect wherever an ordinary English word occurs.
    A gate that cries constantly is a gate somebody switches off.

    ⚠ Counted with `ast` for this file's standing reason: a regex cannot tell a
    key from a sentence about one, and this module's own prose spells several.
    """
    found: list[tuple[str, int, str]] = []
    for path in sources():
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        name = str(path.relative_to(ROOT))
        found.extend((name, line, key) for line, key in config_keys_in(text))
    return sorted(found)


def config_keys_in(source: str) -> list[tuple[int, str]]:
    """`(line, key)` for every sourced configuration key one source spells.

    The needle itself, over a string rather than a path, so the corpus sweep and
    the selftest that proves the needle works are ONE derivation. See
    [`spelled_config_keys`] for what is counted and what is deliberately not.
    """
    needles = {path for path in config_surface_paths() if "." in path}
    try:
        tree = ast.parse(source)
    except SyntaxError:
        # A file this tool cannot parse is a file it must not judge.
        return []
    return sorted(
        (node.lineno, node.value)
        for node in ast.walk(tree)
        if isinstance(node, ast.Constant)
        and isinstance(node.value, str)
        and node.value in needles
    )


# ⚠ The needle above was emptied on purpose at R2156 and the selftest said
# `a walk spelling 'admin.enabled' was not caught` — so its zero on this tree
# is a measurement rather than the silence of a check that looks at nothing.


def read_unpinned() -> set[str]:
    """The converted-and-unpinned families this tree already carries."""
    if not UNPINNED.is_file():
        return set()
    return {
        line.strip()
        for line in UNPINNED.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.startswith("#")
    }


def write_unpinned(stems: Iterable[str]) -> None:
    """Re-pin the list, so its SHRINKING is a diff rather than a claim."""
    lines = [
        "# R2147 — family/ies whose every reader was converted and whose value",
        "# nothing pins: no `.pin` artifact covers them and no assertion spells",
        "# them. Each row is a check this campaign DELETED rather than repaid.",
        "#",
        "# The gate refuses a family JOINING this list, which is what stops a",
        "# conversion from taking the last check with it. It does not refuse the",
        "# rows already here: a gate that is red the day it is written is one",
        "# nobody turns on, and these were created over many rounds.",
        "#",
        "# The fix for a row is to give the family a value pin — a screen's",
        "# address-pin test, or a crate that emits its grammar — and then run",
        "# `python3 tools/painted_addresses.py --write-budget`.",
        "#",
        "# Rewritten by that command; do not hand-edit.",
    ]
    lines += sorted(stems)
    UNPINNED.write_text("\n".join(lines) + "\n", encoding="utf-8")


def unspelled() -> int:
    """Which families no Rust source spells — under each needle, side by side.

    ★★★★★ R2103 — the command that replaces a paragraph of hand-measured
    counts. This question is asked every time somebody proposes telling this
    census's real families from its noise by looking for the stem in Rust; it
    has been answered by hand twice, and once wrongly. The answer also MOVES as
    the debt is repaid — a family converted to a composition the screen
    publishes stops being spelled in Rust too — so it is exactly the kind of
    number that must not sit in prose.

    Prints every family the LITERAL needle calls silent, since that is the
    wider set, and names the ones the two needles disagree about: those are the
    families whose fate under such a filter would be decided by a comment.
    """
    families = sorted(set(read_budget()) | set(census()))
    if not families:
        print("painted-addresses: no families to ask about")
        return 0
    text = rust_text()
    verdicts = {stem: rust_needles(text, stem) for stem in families}
    silent = [s for s in families if not verdicts[s][0]]
    no_literal = [s for s in families if not verdicts[s][1]]
    only_comment = [s for s in no_literal if verdicts[s][0]]
    print(f"{'source':>7}  {'literal':>7}  family")
    for stem in no_literal:
        in_src, in_lit = verdicts[stem]
        print(f"{'yes' if in_src else 'no':>7}  {'yes' if in_lit else 'no':>7}  {stem}")
    print(
        f"of {len(families)} family/ies: {len(silent)} spelled by no Rust source "
        f"at all, {len(no_literal)} by no Rust string literal"
    )
    if only_comment:
        print(
            f"★ the two needles disagree about {len(only_comment)}: "
            f"{', '.join(only_comment)} — Rust's only spelling is a comment, so "
            "a filter built on 'the stem appears in Rust' keeps or deletes "
            "these by accident"
        )
    return 0


def _config_keys_in(source: str) -> list[str]:
    """The sourced config keys one snippet spells, for the selftest.

    ⚠ A caller of [`config_keys_in`], never a second copy of its rule. The
    needle and the thing that exercises the needle sharing one derivation is
    this file's own subject: a gate holding its own copy of the rule it checks
    passes while the real one is wrong, which R2125 paid for one layer up.
    """
    return [key for _line, key in config_keys_in(source)]


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

    # ★★★★★ R2103 — the two Rust needles, against FIXTURES rather than this
    # tree. The prose these replace was measured once against a moving corpus
    # and was wrong by the time it was read; a case pinned to today's
    # `examples/` would rot exactly the same way. What cannot rot is the
    # discrimination itself: a comment is not a string literal.
    needle_cases: list[tuple[str, str, str, tuple[bool, bool]]] = [
        (
            "a stem in a string literal is spelled under both needles",
            'fn f() { press("lab.node.T-01"); }',
            "lab.node",
            (True, True),
        ),
        (
            "★ a stem in a COMMENT is source-only — the case that broke the prose",
            "// the id is `expanded.struct:...` for a struct row\n",
            "expanded.struct",
            (True, False),
        ),
        (
            "a stem nowhere is silent under both",
            'fn f() { press("shell.rail.Packets"); }',
            "nodegroups.node",
            (False, False),
        ),
        (
            "★★ a doc comment in a file that DOES carry literals still separates",
            '/// see `line.area`\nfn f() { press("lab.pin.x.dial"); }',
            "line.area",
            (True, False),
        ),
        (
            "⚠ an escaped quote does not end a literal early and hide the stem",
            r'fn f() { p("a\"b lab.rail.x"); }',
            "lab.rail",
            (True, True),
        ),
    ]
    for label, fixture, stem, want in needle_cases:
        got = rust_needles(fixture, stem)
        if got != want:
            failed += 1
            print(f"FAIL: {label}: rust_needles -> {got}, wanted {want}", file=sys.stderr)

    # ★★★★★ R2144 — the READER/ASSERTION discrimination, against fixtures.
    # The case that matters most is the third: it is the shape that refuted the
    # naive "everything after the first #[cfg(test)]" rule on this tree's own
    # 54 files, and it is what a later simplification would break first.
    role_cases: list[tuple[str, str, tuple[int, int]]] = [
        (
            "a literal in production is a reader",
            'fn view() { tag("lab.node.T-01"); }\n',
            (1, 0),
        ),
        (
            "a literal inside a #[cfg(test)] module is an assertion",
            '#[cfg(test)]\nmod tests {\n  fn t() { assert("lab.node.T-01"); }\n}\n',
            (0, 1),
        ),
        (
            "★★★★★ a production item AFTER the test module is still a READER — "
            "the naive first-marker rule called this an assertion, and 54 files "
            "of this tree have the shape",
            '#[cfg(test)]\nmod tests {\n  fn t() { assert("lab.node.A"); }\n}\n'
            'const W: &str = "lab.node.B";\n',
            (1, 1),
        ),
        (
            "★ a #[cfg(test)] on a single fn covers only that fn",
            '#[cfg(test)]\nfn helper() { tag("lab.node.A"); }\n'
            'fn view() { tag("lab.node.B"); }\n',
            (1, 1),
        ),
        (
            "a comment is neither a reader nor an assertion",
            '// lab.node.T-01 is the card\nfn view() {}\n',
            (0, 0),
        ),
        (
            "two test modules both count as assertions",
            '#[cfg(test)]\nmod a {\n  fn t() { assert("lab.node.A"); }\n}\n'
            '#[cfg(test)]\nmod b {\n  fn t() { assert("lab.node.B"); }\n}\n',
            (0, 2),
        ),
    ]
    for label, fixture, want in role_cases:
        got = rust_roles(Path("fixture.rs"), fixture)
        if got != want:
            failed += 1
            print(f"FAIL: {label}: rust_roles -> {got}, wanted {want}", file=sys.stderr)

    # ★★★★★ R2147 — the rule that says a family's VALUE is held. Against
    # fixtures, because what must not rot is the discrimination: a family
    # wrongly called pinned is one whose last check a later round deletes
    # believing it safe, and that failure is silent.
    covers_cases: list[tuple[str, str, str, bool]] = [
        ("a pin row IS the family", "card.alarms", "card.alarms", True),
        (
            "★ a family INSIDE a whole address is pinned by it — a screen's pin\n"
            "       holds `card.alarms.<i>.feed.head`, and renaming `feed.head`\n"
            "       moves that row",
            "card.alarms.feed.head.col",
            "feed.head",
            True,
        ),
        (
            "★★ and the boundary is load-bearing in the SAFE direction: a longer\n"
            "       word does not answer for a shorter one",
            "card.alarms.feed.header",
            "feed.head",
            False,
        ),
        (
            "a family the pin does not mention is not pinned",
            "card.alarms.feed.head",
            "tv.node",
            False,
        ),
    ]
    for label, address, stem, want in covers_cases:
        if covers(address, stem) is not want:
            failed += 1
            print(
                f"FAIL: {label}: covers({address!r}, {stem!r}) -> "
                f"{covers(address, stem)}, wanted {want}",
                file=sys.stderr,
            )

    # ★★★★★ R2153 — the GRAMMAR pin, and above all the half that says NO.
    #
    # A crate's emitted grammar pins a family under every prefix, which is what
    # `chart.area` had and `line.area` — the same grammar, same artifact — did
    # not, so converting a custom-prefixed chart was reported as deleting a
    # check. Widening a pin test is the dangerous direction, so the second case
    # is the one to keep: the parts a chart declares are ordinary words, and a
    # screen family called `panel.label` must NOT be claimed by an artifact
    # that has never heard of it.
    grammar_cases: list[tuple[str, str, bool]] = [
        ("a declared grammar pins its family under the default prefix", "chart.area", True),
        (
            "★ and under a prefix a chart in this tree is actually given — the\n"
            "       case whose absence charged a repaid family as a deleted check",
            "line.area",
            True,
        ),
        (
            "★★ but NOT under a prefix no chart uses: the parts are ordinary\n"
            "       words, and this is the direction that loses a check silently",
            "panel.label",
            False,
        ),
        ("a part no grammar declares is not pinned by one", "chart.nosuchpart", False),
        ("a bare stem with no part is not pinned", "chart", False),
    ]
    for label, stem, want in grammar_cases:
        if grammar_pins(stem) is not want:
            failed += 1
            print(
                f"FAIL: {label}: grammar_pins({stem!r}) -> {grammar_pins(stem)}, "
                f"wanted {want}",
                file=sys.stderr,
            )
    # And neither half may be empty, or both cases above pass vacuously.
    if not grammar_parts():
        failed += 1
        print("FAIL: no grammar artifact was read — every grammar pin is vacuous", file=sys.stderr)
    if len(grammar_prefixes()) < 2:
        failed += 1
        print(
            f"FAIL: {len(grammar_prefixes())} chart prefix/es found in the source — "
            "the guard that makes a grammar pin safe is not reading anything",
            file=sys.stderr,
        )

    # ★ And the classifier is not a constant. A `pin_sources` that answered one
    # word for everything would make the gate below either always green or
    # always red, and both read as working.
    live_pins = pin_sources(set(read_budget()) | set(census()))
    for source in ("artifact", "assertion", ""):
        if not any(value == source for value in live_pins.values()):
            failed += 1
            print(
                f"FAIL: no family is classified {source or 'unpinned'!r} — the "
                "pin classifier is answering one word for the whole tree",
                file=sys.stderr,
            )
    if len(pin_artifact_addresses()) < 50:
        failed += 1
        print(
            f"FAIL: the pin artifacts hold only "
            f"{len(pin_artifact_addresses())} address(es) — this is not the "
            "corpus, and every family would read as unpinned",
            file=sys.stderr,
        )

    # ★★★★★ R2116 — the RUST census's rule, against fixtures for the same
    # reason: what must not rot is the discrimination, not today's families.
    rust_cases: list[tuple[str, str, list[str]]] = [
        (
            "a whole address in a literal is a site",
            'fn f() { press("lab.reset.nodes"); }',
            ["lab.reset"],
        ),
        (
            "★ a `format!` that spells the prefix is a site — the shape converted",
            'let t = format!("lab.reset.{}", scope.wire());',
            ["lab.reset"],
        ),
        (
            "★ naming the const is NOT a site — the converted shape",
            "let t = address::reset(scope.wire());",
            [],
        ),
        (
            "★★ a DOC COMMENT about an address is not a literal, so not a site",
            "/// painted at `lab.reset.view`, which the panel draws.\nfn f() {}",
            [],
        ),
        (
            "★★ and neither is a line comment",
            'fn f() { /* lab.reset.view */ press(X); }',
            [],
        ),
        (
            "★★★ the needle is anchored: a sentence inside a literal is not a site",
            'panic!("the seat at lab.reset.view is gone");',
            [],
        ),
        (
            "a two-segment prefix is what a screen publishes, not a spelling",
            'const STEM: &str = "lab.reset";',
            [],
        ),
        (
            "two families in one file are two sites",
            'let a = "lab.form.remove.mode";\nlet b = "shell.rail.lab";',
            ["lab.form", "shell.rail"],
        ),
        (
            "an rpc method path is not an address",
            'call("scene/containment");',
            [],
        ),
        (
            "⚠ an escaped quote does not end a literal early and split the next",
            r'let a = "he said \"go\""; let b = "lab.pin.P-01.dial";',
            ["lab.pin"],
        ),
    ]
    for label, fixture, want in rust_cases:
        got = sorted(stem for _, stem in rust_sites_in(fixture))
        if got != sorted(want):
            failed += 1
            print(f"FAIL: {label}: want {want}, got {got}", file=sys.stderr)

    # ★★★★★ And the RUST POPULATION, asserted rather than assumed — the same
    # class the oracle case below is about. A `rglob` that matched nothing, or
    # an exclusion that matched everything, both answer zero and read as a pass.
    # ★★★★★ R2156 — the config-key needle, which answers ZERO on this tree and
    # so proves nothing by running. A gate whose only observed state is empty is
    # indistinguishable from one that looks at nothing, and this file has
    # already recorded that shape twice. So the needle is exercised against a
    # source that DOES carry a key, and against one that carries the single-word
    # paths it must decline.
    surface = config_surface_paths()
    dotted = [path for path in surface if "." in path]
    single = [path for path in surface if "." not in path]
    if not dotted or not single:
        failed += 1
        print(
            f"FAIL: the config surface offers {len(dotted)} dotted and "
            f"{len(single)} single-word path(s); the needle's two arms cannot "
            "both be exercised, so one of them is untested",
            file=sys.stderr,
        )
    else:
        caught = _config_keys_in(f'row(tf, "{dotted[0]}")\n')
        if caught != [dotted[0]]:
            failed += 1
            print(
                f"FAIL: a walk spelling {dotted[0]!r} was not caught: {caught}",
                file=sys.stderr,
            )
        # ⚠ The other arm. `id` and `mode` are sourced paths AND ordinary
        # English; a needle over them reports a defect wherever the word occurs.
        declined = _config_keys_in(f'press(tf, "{single[0]}")\n')
        if declined:
            failed += 1
            print(
                f"FAIL: the single-word path {single[0]!r} was counted as a "
                f"spelled key: {declined} — an ordinary word is not a key",
                file=sys.stderr,
            )
        # ⚠ And a MENTION is not a site: a comment is not in the tree at all.
        mentioned = _config_keys_in(f"# this walk is about {dotted[0]}\n")
        if mentioned:
            failed += 1
            print(
                f"FAIL: a comment naming {dotted[0]!r} was counted: {mentioned}",
                file=sys.stderr,
            )

    rust_pop = rust_sources()
    crates_seen = {
        path.relative_to(ROOT / RUST_CENSUS_ROOT).parts[0] for path in rust_pop
    }
    # ★ R2155 — EVERY declaring name, not just the first. An exclusion added to
    # the set and never present in the tree would be a filter matching nothing,
    # which is the case this block exists to refuse, and checking only the first
    # name would have let the second in silently.
    declaring = sorted(
        path
        for name in RUST_DECLARING_FILES
        for path in (ROOT / RUST_CENSUS_ROOT).glob(f"*/src/**/{name}")
    )
    unfound = [
        name
        for name in RUST_DECLARING_FILES
        if not any(path.name == name for path in declaring)
    ]
    if unfound:
        failed += 1
        print(
            f"FAIL: no {unfound} exists under {RUST_CENSUS_ROOT}/, so an "
            "exclusion that keeps a declaring site from being charged for "
            "declaring matches nothing and is not doing anything",
            file=sys.stderr,
        )
    if len(crates_seen) < 2:
        failed += 1
        print(
            f"FAIL: the Rust census population spans {len(crates_seen)} crate(s) "
            f"under {RUST_CENSUS_ROOT}/ — it is reading less than the tree holds, "
            "and a census that sees nothing answers zero about everything",
            file=sys.stderr,
        )
    if not declaring:
        failed += 1
        print(
            f"FAIL: no `{RUST_DECLARING_FILE}` exists under {RUST_CENSUS_ROOT}/, so "
            "the exclusion that keeps a declaring site from being charged for "
            "declaring matches nothing and is not doing anything",
            file=sys.stderr,
        )
    elif any(path in rust_pop for path in declaring):
        failed += 1
        print(
            "FAIL: a declaring module is inside the Rust census population — "
            "every screen would be charged for the file that fixes the defect",
            file=sys.stderr,
        )

    # ★★★★★ R2103 — and the ORACLE, against this repository's real Rust.
    #
    # Every case above hands the pure rule a fixture, which is what keeps them
    # from rotting — and leaves [`rust_text`] exercised by nothing. A reader
    # that found no files at all would then answer *spelled nowhere* for every
    # family, `--unspelled` would report the whole census silent, and all five
    # cases above would still pass. That is this workspace's registered class:
    # a pure rule is remembered and the oracle that hands it the world is not.
    #
    # So the expectation is DERIVED FROM THE TREE by a different method than
    # the one under test — a per-file read that stops at the first Rust literal
    # spelling an address, against the joined corpus `rust_text` builds. Naming
    # a family here would be a hand-written list, and it would rot the moment
    # that family converted.
    seed = ""
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            body = path.read_text(encoding="utf-8", errors="replace")
            for literal in RUST_LITERAL.findall(body):
                hit = ADDRESS.match(literal[1:])
                if hit:
                    seed = hit.group(1)
                    break
            if seed:
                break
        if seed:
            break
    if not seed:
        failed += 1
        print(
            "FAIL: no Rust string literal spells an address anywhere, so the "
            "oracle case below cannot fail and is not checking anything",
            file=sys.stderr,
        )
    elif rust_needles(rust_text(), seed) != (True, True):
        failed += 1
        print(
            f"FAIL: the corpus rust_text() builds does not carry {seed!r}, which "
            "a file-by-file read of the same trees found in a literal — the "
            "oracle is reading less than the tree holds",
            file=sys.stderr,
        )

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
    rust_now = rust_census()
    budget = read_budget()

    # A family the budget pins at zero must really be spelled nowhere. This is
    # what makes a conversion durable: R2049-R2053 converted five families with
    # nothing behind them but a Rust test that could not see a walk.
    broken = [
        (s, now.get(s, 0), rust_now.get(s, 0))
        for s, n in budget.items()
        if n.walk == 0 and n.rust == 0 and (now.get(s, 0) or rust_now.get(s, 0))
    ]
    if broken:
        failed += 1
        print(
            f"FAIL: family/ies pinned at zero are spelled again "
            f"(stem, walk, rust): {broken}",
            file=sys.stderr,
        )

    # A budget describing a tree that has moved on is a budget nobody can read.
    if budget:
        stale = [s for s in set(now) | set(rust_now) if s not in budget]
        if stale:
            failed += 1
            print(
                f"FAIL: the census finds family/ies the budget has never seen: "
                f"{sorted(stale)}",
                file=sys.stderr,
            )
        gone = [
            s
            for s, n in budget.items()
            if (n.walk and s not in now) or (n.rust and s not in rust_now)
        ]
        if gone:
            failed += 1
            print(
                f"FAIL: the budget charges family/ies a census cannot find — "
                f"they were converted and never re-pinned: {sorted(gone)}. Run "
                f"--write-budget.",
                file=sys.stderr,
            )
        # ★★★★★ R2116 — every budget row carries BOTH columns. A row left in the
        # single-column shape reads as `rust = 0`, which is a claim about a
        # population nobody measured, and it is the shape this format replaced.
        thin = [
            line.split("\t")[0]
            for line in BUDGET.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#") and line.count("\t") != 2
        ]
        if thin:
            failed += 1
            print(
                f"FAIL: budget row(s) carry one column and so claim a Rust count "
                f"nobody measured: {thin}. Run --write-budget.",
                file=sys.stderr,
            )

    # ⚠ R2144 — this sum is HAND-MAINTAINED, and adding a case list without
    # adding it here leaves the printed number covering less than it claims:
    # the six `role_cases` ran for one edit while the line still said 39. The
    # trailing `+ 10` is a pre-existing constant for the ad-hoc assertions
    # above; it is left alone rather than guessed at.
    total = (
        len(cases)
        + len(needle_cases)
        + len(rust_cases)
        + len(role_cases)
        + len(covers_cases)
        + 4  # R2147: three classifier words and the artifact corpus floor
        + 10
    )
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
    parser.add_argument(
        "--unspelled",
        action="store_true",
        help="which families no Rust source spells, under each of the two needles",
    )
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.write_budget:
        write_budget(census(), rust_census(), pin=args.pin)
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
    if args.unspelled:
        return unspelled()
    if args.list:
        walk = scan().get(args.list, [])
        rust = rust_scan().get(args.list, [])
        for path, line in walk + rust:
            print(f"{path}:{line}")
        print(
            f"{len(walk) + len(rust)} site(s) under {args.list} "
            f"({len(walk)} walk, {len(rust)} rust)"
        )
        return 0
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
