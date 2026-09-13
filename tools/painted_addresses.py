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
all. The selftest pins both. ★★★★★ R2189 — or at the start of the path behind a
MOUNT: `/external/node.0.x` is read as `node.0.x` (see `address_part`), because an
RPC string that mounts a path spells that path.

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
import bisect
import functools
import json
import re
import sys
from pathlib import Path
from typing import Callable, Iterable, NamedTuple

sys.path.insert(0, str(Path(__file__).resolve().parent))

import painted_grammar  # noqa: E402  — the one reader of the emitted artifact

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
#:
#: ★★★★★ R2181 — **the family segment may carry an INSTANCE KEY**, and the stem
#: drops it. A placed card is painted at `card.alarms#6.feed`: the kind, then
#: its place on the board. This shape read the `#` as the end of the match and
#: saw no address at all, so the shell's whole card vocabulary was invisible —
#: concrete or templated, reader or assertion. Measured before writing: 26 walk
#: readers, 17 Rust declarations and 31 assertions, in families whose value the
#: shell's own `.pin` already holds (it folds the key as `card.alarms#*`, which
#: is the same family by the same rule).
ADDRESS = re.compile(r"^([a-z][a-z0-9_]*\.[a-z][a-z0-9_]*)(?:#(?:\d+|\{[^{}]*\}))?\.")

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
    """`(line, family stem)` for every spelled painted address in `path`.

    ★★★★★ R2181 — **a VIEW of [`_walk_literals`]**, which is the walk half's
    one needle now. This read an f-string's constant HALVES and anchored each
    one, while `_walk_literals` anchored the whole template, so the walk corpus
    had two populations that nothing compared. Measured before merging: they
    disagreed in 2 of 731 files, three sites, all
    `f"{parts['item']}listen.endpoints.add"` — a prefix handed over and a
    configuration key spelled after it, which the halves rule anchored at
    `listen.endpoints` in the middle of a literal. The module header's claim is
    that the needle is anchored at the START of a literal; that was true of one
    reader and not the other. Those three are now counted where their family
    really is unknown, by [`unanchored_reader_sites`].
    """
    return sorted((line, family_of(literal)) for line, literal in _walk_literals(path))


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

    A literal is a site when the address is ANCHORED at its start (or at the start
    of the path behind a mount, R2189's [`address_part`]) — the same rule
    the Python half uses, and for the same reason. A Rust doc comment is not a
    literal, so prose is out of this population; what covers prose is each
    screen's own gate, whose needle is the bare stem.

    ⚠ [`RUST_LITERAL`] is approximate in one direction only (a raw string reads
    as an ordinary one; a comment carrying a lone quote can open one), so this
    can over-count and cannot under-count. Against a ratchet an over-count is a
    stable offset rather than a hole: it makes the pinned number larger than the
    truth, and a family whose real count RISES still rises here.

    ★★★★★ R2170 — **a COMMENT line is not a site, and that rule lives here
    now.** It was written in [`rust_site_roles`] and not in this needle, so the
    two populations disagreed: measured, the census counted 378 Rust sites
    while only 372 of them could be given a role, the other six being prose —
    four doc comments naming `echo.demo.echo`, one naming `modified.elem.2`,
    and one a comment R2170 itself wrote while converting a site. A rule that
    holds for one caller and not the needle is this session's recurring defect;
    the needle owns it, and the arithmetic is gated in [`selftest`].
    """
    return [(line, stem) for line, stem, _literal in rust_site_literals(text)]


#: ★★★★★ R2178 — the calls whose first argument is a name in ANOTHER namespace:
#: the reactive owner's cache (`Owner::cache`, `cache_get_by_str`,
#: `cache_contains`), which names a state slot, not a painted mark.
#:
#: Measured before this was written, so it is a class and not an exception: 167
#: owner-cache keys in the Rust tree, exactly two dotted —
#: `settings_panel.persistence.boot` and `todomvc.persistence.boot` — and the
#: census counted both as READERS of a painted address and reported both
#: BLOCKED, i.e. half of the four sites this campaign called work no round may
#: finish. Of the 365 Rust census sites, grouped by the call each literal is
#: handed to, these were the only reader-role literals in a non-paint API. The
#: other context with volume, 45 literals handed to `query` / `intervene`, is
#: already the `$schema` path vocabulary (R2166) and every one is an assertion.
#:
#: ⚠ A NAME rule, stated with its limit: a METHOD called `cache` on something
#: other than an owner would also be read as a slot key. The dot is required,
#: so a free function called `cache` is not, and a UFCS call
#: `Owner::cache(&o, "k")` is not either — both err towards COUNTING, which is
#: the direction this census is allowed to err in (see [`RUST_LITERAL`]).
NON_ADDRESS_CALLS = frozenset({"cache", "cache_get_by_str", "cache_contains"})

def handed_to(text: str, start: int) -> str | None:
    """The method the literal beginning at `start` is the first argument of.

    ★★★★★ R2178 — R2167's `site_vocabulary` recorded its own limit: *a tighter
    derivation needs each SITE's namespace — the reader it is handed to — and
    that is the next instalment.* This is that reader, narrowed to what was
    measured to matter. PURE, and handed text rather than a path, so every case
    is a fixture.

    ⚠ A backward SCANNER, not a regex, and the first draft is why. It read the
    turbofish as `::<[^>]*>`, which stops at the first `>` — so
    `cache::<Rc<State>, _>("a.b.c")` was not recognised and its key was counted
    as an address. A regex can only raise the nesting depth it survives; a
    bracket counter has none.

    Reads, right to left: whitespace, `(`, whitespace, an optional balanced
    `<...>` preceded by `::`, the method name, and a `.` — the dot is required,
    so a free function or a UFCS call answers `None`.
    """
    i = start - 1
    while i >= 0 and text[i].isspace():
        i -= 1
    if i < 0 or text[i] != "(":
        return None
    i -= 1
    while i >= 0 and text[i].isspace():
        i -= 1
    if i >= 0 and text[i] == ">":
        depth = 0
        while i >= 0:
            if text[i] == ">":
                depth += 1
            elif text[i] == "<":
                depth -= 1
                if depth == 0:
                    break
            i -= 1
        if i < 2 or text[i - 2 : i] != "::":
            return None
        i -= 3
    end = i + 1
    while i >= 0 and (text[i].isalnum() or text[i] == "_"):
        i -= 1
    name = text[i + 1 : end]
    if not name or not (name[0].isalpha() or name[0] == "_"):
        return None
    return name if i >= 0 and text[i] == "." else None


#: A Rust `&str` constant or static bound to one brace-free literal on one line —
#: the value a `{NAME}` placeholder in a format string captures.
RUST_STR_CONST = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Z_][A-Z0-9_]*)\s*:\s*"
    r"&(?:'static\s+)?str\s*=\s*\"([^\"\\{}]*)\"\s*;",
    re.MULTILINE,
)

#: A placeholder that NAMES something — `{CHART_TAG}`, never `{}` or `{n:>3}`.
_NAMED_PLACEHOLDER = re.compile(r"\{([A-Za-z_][A-Za-z0-9_]*)\}")


def rust_str_constants(text: str) -> dict[str, str]:
    """`NAME -> value` for every `&str` const or static `text` binds to ONE value.

    ★★★★★ R2181 — a format string captures a name inline, so
    `format!("{CHART_TAG}.label.x.{k}")` beside `const CHART_TAG: &str = "chart"`
    spells `chart.label.x.{k}` exactly as the literal would. The needle read the
    brace and saw no address. Measured before writing: 10 such literals anchor
    once the constant is substituted.

    ⚠ A name bound to two DIFFERENT values in one file (a `TAG` in each of two
    modules) is dropped: which one a placeholder captures is a scoping question
    this does not answer, and leaving the brace in place errs towards NOT
    anchoring — which [`unanchored_reader_sites`] still counts. A value holding a
    brace is never substituted either, because the brace it would insert is text
    at runtime and a placeholder to this census.
    """
    values: dict[str, set[str]] = {}
    for name, value in RUST_STR_CONST.findall(text):
        values.setdefault(name, set()).add(value)
    return {name: next(iter(v)) for name, v in values.items() if len(v) == 1}


def resolve_placeholders(literal: str, constants: dict[str, str]) -> str:
    """`literal` with every placeholder naming one of `constants` replaced by its
    value, and every other placeholder left as it is."""
    return _NAMED_PLACEHOLDER.sub(
        lambda match: constants.get(match.group(1), match.group(0)), literal
    )


@functools.lru_cache(maxsize=None)
def rust_literals(text: str) -> tuple[tuple[int, int, str], ...]:
    """`(offset, line, literal)` for every Rust string literal outside a comment
    line, quotes stripped and constant placeholders resolved — **the one scan**
    every Rust needle in this file reads.

    ★★★★★ R2181 — [`rust_site_literals`] and [`non_address_literals`] each
    walked [`RUST_LITERAL`] and each re-applied the comment rule, and a third
    reader was about to join them. Resolving a constant in two of three copies
    would have made the populations disagree on purpose, which is R2178's
    finding one layer down. `offset` is the opening quote, which is what
    [`handed_to`] reads back from.

    ★★★★★ R2183 — **linear, and read once per source.** Each literal's line was
    `text.count("\n", 0, offset)`, a rescan from the top of the file per
    literal, and every derivation in this file re-ran the scan on the same text:
    profiled at R2183, `--check` spent 71.6 s of 109.3 s in `str.count`, over
    4,388 calls of this function for ~1,500 files, and the push gate's census
    step took 158.6 s. The line is now a bisection over the newline offsets —
    the same count, taken once — and the result is cached by source text, the
    arrangement [`python_literals`] has had since R2181. The comment rule still
    indexes `splitlines`, exactly as before, so no output can move.
    """
    lines = text.splitlines()
    constants = rust_str_constants(text)
    line_starts = [0] + [match.end() for match in re.finditer("\n", text)]
    found: list[tuple[int, int, str]] = []
    for match in RUST_LITERAL.finditer(text):
        line = bisect.bisect_right(line_starts, match.start())
        if lines[line - 1].lstrip().startswith("//"):
            continue
        found.append(
            (match.start(), line, resolve_placeholders(match.group(0)[1:-1], constants))
        )
    return tuple(found)


def rust_site_literals(text: str) -> list[tuple[int, str, str]]:
    """`(line, family stem, literal)` for every painted address Rust `text`
    spells — **THE needle**, and the only one.

    ★★★★★ R2178 — this census had THREE copies of its Rust literal loop: this
    one, [`rust_reader_duplication`] (R2168) and the Rust half of
    `address_spellings` (R2168, now [`address_literal_sites`]). Each re-spelled
    the comment rule R2170 moved
    here, and the third only agreed with the needle by accident — it looked a
    role up by LINE, so a non-address literal sharing a line with a real tag
    would have been counted as a reader. R2175 named the copy and deferred it
    because the repair needed the literal this needle dropped. Adding a SECOND
    population rule to a needle with live copies would have made that
    divergence deliberate, so the literal is carried now and the copies derive
    from it.

    Two population rules, both exact rather than guessed from shape (R2103's
    refusal to guess stands):

    * a line whose first non-space is `//` is prose about an address (R2170);
    * a literal handed as the key of a [`NON_ADDRESS_CALLS`] method names a
      state slot, not a mark (R2178). Those are not dropped silently —
      [`non_address_literals`] counts them.

    ★★★★★ R2181 — the literal is the one [`rust_literals`] hands over, so a
    placeholder naming a same-file constant is read as that constant's value
    and the literal carried is the address it spells.
    """
    found: list[tuple[int, str, str]] = []
    for offset, line, literal in rust_literals(text):
        stem = family_of(literal)
        if stem and handed_to(text, offset) not in NON_ADDRESS_CALLS:
            found.append((line, stem, address_part(literal)))
    return sorted(found)


def non_address_literals(text: str) -> list[tuple[int, str, str]]:
    """`(line, stem, method)` for every literal with an address's SHAPE that is
    handed to a [`NON_ADDRESS_CALLS`] method — what [`rust_site_literals`]
    excludes, counted rather than dropped.

    ★ R2160's rule: a census that drops its hard part measures comfort instead
    of remainder. An exclusion nobody can count is exactly that.
    """
    found: list[tuple[int, str, str]] = []
    for offset, line, literal in rust_literals(text):
        stem = family_of(literal)
        if not stem:
            continue
        method = handed_to(text, offset)
        if method in NON_ADDRESS_CALLS:
            found.append((line, stem, method))
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


@functools.lru_cache(maxsize=1)
def test_only_modules() -> frozenset[Path]:
    """Every module file that its PARENT declares behind `#[cfg(test)]`.

    ★★★★★ R2158 — **the classifier could not see a test module whose marker is
    in another file, and 43 assertions were being counted as readers.**

    [`test_spans`] finds `#[cfg(test)]` inside the text it is given, which is
    right for an inline `mod tests {…}` and blind to the other arrangement this
    workspace uses just as much: `#[cfg(test)] mod tests;` in the parent, with
    the whole of `tests.rs` behind it. Measured at R2158, **20 module files** are
    declared that way — every screen's `tests.rs` and six screens' `painted.rs`
    — and every site in them was scored as a production reader.

    ⚠⚠ That is not a rounding error in the campaign's number, it is a wrong
    INSTRUCTION. This debt's closing criterion is *reducible = walk + Rust
    readers = 0*, and the debt's own text says assertions are welcome and are
    the only thing pinning a family's address VALUES. So the instrument was
    telling the campaign to convert the very checks the debt says to keep — and
    a round that obeyed would have reported repayment while deleting pins.

    ⚠ Exact rather than heuristic, which is what makes it safe to move sites in
    the direction that LOWERS the reader count: a file is here only because its
    parent literally declares it `#[cfg(test)] mod <name>;`. R2144's warning was
    about a rule that took everything after the first marker in a file and so
    swept up production items; this sweeps up nothing it was not told about by
    name.
    """
    found: set[Path] = set()
    for root in (RUST_CENSUS_ROOT, "crates"):
        for parent in sorted((ROOT / root).glob("*/src/**/*.rs")):
            try:
                text = parent.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for name in re.findall(
                r"#\[cfg\(test\)\]\s*\n\s*mod\s+([a-z_0-9]+)\s*;", text
            ):
                found.add(parent.parent / f"{name}.rs")
                found.add(parent.parent / name / "mod.rs")
    return frozenset(found)


#: How a Rust item BINDS a value to a name — `const X: &str = …`, `static Y: …`.
CONST_ITEM = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Z_][A-Z0-9_]*)\s*:"
)


def const_items(text: str) -> dict[str, set[int]]:
    """`const`/`static` name -> the 1-based lines its initializer covers.

    PURE, and handed its text rather than a path for [`rust_sites_in`]'s reason:
    what has to be tested is the discrimination, not today's files.

    The span is closed by bracket depth rather than by a `;` alone, because the
    shape this exists for is a multi-line array — `const HOVER_KEYS: [&str; 6]`
    holds six addresses on six lines and every one of them is inside the item.
    """
    found: dict[str, set[int]] = {}
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        match = CONST_ITEM.match(lines[index])
        if not match:
            index += 1
            continue
        depth = 0
        cursor = index
        span: set[int] = set()
        while cursor < len(lines):
            line = lines[cursor]
            depth += line.count("[") + line.count("(") + line.count("{")
            depth -= line.count("]") + line.count(")") + line.count("}")
            span.add(cursor + 1)
            if line.rstrip().endswith(";") and depth <= 0:
                break
            cursor += 1
        found.setdefault(match.group(1), set()).update(span)
        index = cursor + 1
    return found


#: A function header, and the return type that makes it an address COMPOSER.
#:
#: ★★★★★ R2170 — R2169 recognised a `const` binding and stopped there, and a
#: composer FUNCTION is the same declaration in another syntactic form:
#: `fn cell_tag(r, c) -> String { format!("dev.cell.{r}.{c}") }` has three
#: consumers and a test pinning its value, and its one `format!` was still
#: billed as a reader owing conversion.
#:
#: ⚠ The RETURN TYPE is the discriminator, and it was chosen over a size limit
#: because a size limit is a magic number. `card_scene(...) -> Scene` happens to
#: tag a node and is an ordinary reader; `block_tag(i) -> &'static str` exists
#: to make the address. What a function RETURNS says which it is.
#:
#: ★★★★★ R2190 — a string may come back WRAPPED. `selected_node_path(..) ->
#: Option<String>` in `hello-node-editor` composes `node.<id>.<field>` for the
#: Details panel's read and its write, and was billed as a reader; the
#: two-segment rule measured next would bill `hello-packet-view`'s
#: `word() -> Option<String>` the same way. Measured over the enclosing return
#: type of every Rust reader, now and under that rule, before widening:
#: `Option` and `Result` of a string are the only wrapped string returns that
#: enclose one. ⚠ And the return type is NOT enough, which widening it exposed:
#: `read_kind_at(..) -> Option<String>` in `hello-inspector` hands
#: `format!("kind.{i}")` to `intro.query` and returns what it READ. A composer
#: returns the address; a function that hands it to a call reads through it —
#: [`declaring_lines`] asks [`call_argument`] which one a line is.
FN_HEADER = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+([a-z_][A-Za-z0-9_]*)\s*[(<]"
)
FN_RETURNS_STRING = re.compile(
    r"->\s*(?:(?:Option|Result)<\s*)?(?:&(?:'[A-Za-z_]+\s+)?str|String)"
    r"(?:\s*>|\s*,[^{]*>)?\s*\{"
)

#: The `format!(` or `format_args!(` a literal opens, optionally borrowed — what
#: [`call_argument`] reads through, right to left from the literal.
_MACRO_OPENING = re.compile(r"(?:&\s*)?\b(?:format|format_args)!\s*\(\s*$")


def call_argument(text: str, start: int) -> str | None:
    """The method the literal at `start` is handed to — directly, or as the
    first argument of a `format!` that is itself handed to one.

    ★★★★★ R2190 — [`handed_to`] reads `.query("a.b.c")` and answers `None` for
    `.query(&format!("a.b.{i}"))`, because the token before that literal is the
    macro's own `(`. That was enough for the owner-cache keys it was built for;
    it is not enough to tell a composer from a function that reads through an
    address, where the address is nearly always formatted. PURE, like
    [`handed_to`], so every case is a fixture.
    """
    direct = handed_to(text, start)
    if direct is not None:
        return direct
    window = max(0, start - 64)
    opening = _MACRO_OPENING.search(text[window:start])
    if opening is None:
        return None
    return handed_to(text, window + opening.start())


def composer_items(text: str) -> dict[str, set[int]]:
    """`fn` name -> the lines of every function that RETURNS a string.

    PURE, like [`const_items`], and closed by brace depth. A function whose
    return type is not a string is not a composer however many addresses it
    mentions — see [`FN_HEADER`] for why the type rather than the size.
    """
    found: dict[str, set[int]] = {}
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        match = FN_HEADER.match(lines[index])
        if not match:
            index += 1
            continue
        opening = index
        while opening < len(lines) and "{" not in lines[opening]:
            opening += 1
        if opening >= len(lines):
            break
        signature = " ".join(line.strip() for line in lines[index : opening + 1])
        depth = 0
        cursor = opening
        span: set[int] = set()
        while cursor < len(lines):
            depth += lines[cursor].count("{") - lines[cursor].count("}")
            span.add(cursor + 1)
            if depth <= 0:
                break
            cursor += 1
        if FN_RETURNS_STRING.search(signature):
            found.setdefault(match.group(1), set()).update(span)
        index = cursor + 1
    return found


def declaring_lines(text: str, is_used: Callable[[str], bool]) -> set[int]:
    """The lines of `text` that BIND an address to a name other code calls.

    ★★★★★ R2169 — **this census knew a declaring FILE and not a declaring
    SITE**, and R2168 measured what that cost: 46 Rust sites it counted as
    readers owing conversion are `const`/`static` items — `const HOVER_KEYS`
    in three examples, the tray's item keys, the legend consts — each one the
    single name the rest of the code calls. Converting them is not possible:
    there is nothing above a const to call, and the only way to make the site
    disappear was to move the line into a file named `address.rs`, which is
    what [`RUST_DECLARING_FILES`] already excuses.

    ⇒ a `const` IS the repair this campaign asks for — one name, one place — so
    it is recognised where it lives instead of being charged as debt.

    ⚠⚠ BOTH HALVES ARE REQUIRED, and the second is what keeps this safe: the
    name must be used somewhere beyond its own declaration. A const nobody
    calls is dead code, not a declaration, and excusing one would hide a family
    whose address nothing reaches. Measured at R2169 over all 46: every one is
    used elsewhere in its package, and none is dead.

    ⚠ This does NOT excuse a const that other sites ALSO spell. The const is the
    declaration; the sites that spell it anyway are the readers, and they stay
    in the queue — which is the whole point of charging the right column.

    ★★★★★ R2190 — a composer's line whose address is HANDED to a call is not
    part of the declaration: that function reads through the address and
    returns what it read ([`FN_RETURNS_STRING`] records the case that showed
    it). A const cannot hand anything to a call, so only composers are asked.
    """
    handed = {
        line
        for offset, line, literal in rust_literals(text)
        if (family_of(literal) or unanchored_shape(literal))
        and call_argument(text, offset) is not None
    }
    lines: set[int] = set()
    for name, span in const_items(text).items():
        if is_used(name):
            lines |= span
    for name, span in composer_items(text).items():
        if is_used(name):
            lines |= span - handed
    return lines


def rust_site_roles(path: Path, text: str) -> list[tuple[int, str, str]]:
    """`(line, family stem, role)` for every spelled site in one Rust file,
    where role is `"reader"` or `"assertion"`.

    ★★★★★ R2164 — **THE one place that says what a Rust site IS**, because the
    census used to say it in two and the two disagreed about the same line.

    R2158 measured that a file the parent declares behind `#[cfg(test)]` is
    assertions throughout, and taught [`rust_roles`] so — which is how the floor
    of 290 is computed. [`asserted_families`], which decides whether a family's
    VALUE is held, kept its own copy of the rule: it required a literal
    `#[cfg(test)]` *inside* the file and looked only at [`test_spans`]. So a
    line in `examples/hello-topology/src/tests.rs` counted as an assertion when
    the census tallied roles and as nothing at all when the census asked what
    pinned the family — and the second answer is the one that labels a family
    BLOCKED, this file's word for *work no round may finish*.

    Measured at R2164: **seven of the twenty-one BLOCKED families are asserted
    in a test-only module**, carrying 7 walk and 32 Rust sites — 39 of the 79
    BLOCKED sites were not blocked at all.

    ⇒ the two callers keep their own POPULATIONS, which differ on purpose (the
    ratchet excludes declaring files; the pin question is asked over the wide
    corpus), and share the ROLE, which never should have differed.

    A comment line is neither role and is dropped: it is prose about an address,
    which [`rust_sites_in`] already declines to treat as a literal.
    """
    role_at = site_role_at(path, text)
    return [(line, stem, role_at(line)) for line, stem in rust_sites_in(text)]


def site_role_at(path: Path, text: str):
    """The role rule for one Rust file, as a function of a site's LINE.

    ★★★★★ R2178 — split out of [`rust_site_roles`] so a caller that needs the
    literal as well as the role pairs them STRUCTURALLY. Until this round the
    only way to get both was to look a role up by line in a dict built from
    `rust_site_roles`, which is how [`rust_reader_duplication`] did it — and a
    dict keyed by line cannot tell two literals on one line apart, so it agreed
    with the needle by accident.

    ⚠ The comment-line skip that stood in `rust_site_roles` is gone, and that is
    not a loosening: the needle drops comment lines before a role is ever asked
    (R2170), so the skip could never fire. A rule written in two places is the
    shape R2164 removed from this very function.
    """
    whole_file_is_test = path in test_only_modules()
    spans = test_spans(text)
    # ★ R2169 — a third role. An ASSERTION still wins, so the floor of 290 is
    # untouched by construction: what this carves out is part of the READER
    # half, which is the half that was charging declarations as debt.
    declaring = declaring_lines(text, lambda name: _name_is_used(path, name))

    def role_at(line: int) -> str:
        if whole_file_is_test or any(first <= line <= last for first, last in spans):
            return "assertion"
        if line in declaring:
            return "declaration"
        return "reader"

    return role_at


@functools.lru_cache(maxsize=None)
def _package_text(package: str) -> str:
    """Every Rust source of one package, concatenated — the corpus a const's
    name is looked for in."""
    blobs: list[str] = []
    for root in RUST_ROOTS:
        base = ROOT / root / package
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            try:
                blobs.append(path.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
    return "\n".join(blobs)


def _package_of(path: Path) -> str:
    """The package directory a Rust source belongs to, or `""`."""
    try:
        parts = path.relative_to(ROOT).parts
    except ValueError:
        return ""
    return parts[1] if len(parts) > 1 and parts[0] in RUST_ROOTS else ""


def _name_is_used(path: Path, name: str) -> bool:
    """Whether `name` appears in its package beyond its own declaration."""
    package = _package_of(path)
    if not package:
        return False
    return _package_text(package).count(name) > 1


def rust_roles(path: Path, text: str) -> tuple[int, int, int]:
    """`(reader, assertion, declaration)` counts for one Rust file's sites."""
    roles = [role for _line, _stem, role in rust_site_roles(path, text)]
    return roles.count("reader"), roles.count("assertion"), roles.count("declaration")


def rust_role_totals() -> tuple[int, int, int]:
    """`(reader, assertion, declaration)` over the whole Rust population."""
    reader = assertion = declaration = 0
    for path in rust_sources():
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        one, two, three = rust_roles(path, body)
        reader += one
        assertion += two
        declaration += three
    return reader, assertion, declaration


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


def public_declaring_modules() -> list[str]:
    """Declaring modules a BINARY-only example publishes with `pub mod`.

    ★★★★★ R2176 — **`pub` in a binary turns the compiler off as a gate, and
    this campaign leans on it.** R2158 ruled that a composer with no production
    consumer is not a declaration but something this campaign made that will
    rot, and the thing that enforces the ruling is `dead_code`: R2171 wrote two
    such composers in `hello-analyzer-shell` and the compiler refused them on
    the spot. R2174 wrote one in `hello-node-groups` and shipped it, because
    that round declared the module `pub mod address;` — and a `pub` item in a
    crate with no library target is reachable-by-declaration, so the lint says
    nothing. Flipping the one word made the compiler name `node_id` in a single
    run.

    A `lib.rs` crate is NOT reported: there `pub` is what the crate's own tests
    and its binary reach the module through, and the lint has a real answer.
    So the rule is exactly *a declaring module declared from `main.rs` in a
    directory with no `lib.rs` must be private*, which is what the seven other
    examples already do and what one round diverged from.

    ⚠ This is a gate about the COMPILER, living in the census, because the
    census is what excuses [`RUST_DECLARING_FILES`] from being charged. An
    excused file whose contents nothing checks is a blanket exemption, which is
    the shape this whole campaign exists to remove.
    """
    stems = {name.removesuffix(".rs") for name in RUST_DECLARING_FILES}
    found: list[str] = []
    for main in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/src/main.rs")):
        if (main.parent / "lib.rs").exists():
            continue
        try:
            text = main.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for line, body in enumerate(text.splitlines(), 1):
            stripped = body.strip()
            for stem in stems:
                if stripped == f"pub mod {stem};":
                    found.append(f"{main.relative_to(ROOT)}:{line} {stripped}")
    return sorted(found)


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


@functools.lru_cache(maxsize=1)
def rust_reader_scan() -> dict[str, tuple[tuple[str, int, str], ...]]:
    """Every Rust READER site, indexed by family stem.

    ★★★★★ R2175 — **the census had a queue and no way to ask which family a
    piece of it was in.** The reducible queue — the number this debt's closing
    criterion reads — is *walk sites plus Rust READERS*, and it was computed as
    a grand total by [`rust_role_totals`] while every per-family question went
    to [`rust_scan`], which counts assertions and declarations too. Two
    populations, and no derivation joining them.

    What that cost, measured at R2175: [`blocked_families`] said its subject was
    "every family that HAS READERS and no pin", filtered on *has any site*, and
    reported *all* its sites. **19 of the 23 sites it called BLOCKED were
    declarations** — lines already in the one home this campaign exists to give
    them — and five of its nine families had no reducible site at all. The gate
    beside it could not catch that, because it re-spelled the rule with the same
    wrong population.

    ⚠ This is R2164's finding again (*39 of 79 BLOCKED sites were not blocked at
    all*), with a different cause. Two rounds of the same sentence is what says
    the repair belongs at the derivation rather than at the cause.
    """
    # ★★★★★ R2178 — each reader carries its LITERAL, paired with its role
    # through `site_role_at` rather than looked up by line, so the duplication
    # split below can be derived from here instead of re-walking the corpus.
    index: dict[str, list[tuple[str, int, str]]] = {}
    for path in rust_sources():
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        name = str(path.relative_to(ROOT))
        role_at = site_role_at(path, body)
        for line, stem, literal in rust_site_literals(body):
            if role_at(line) == "reader":
                index.setdefault(stem, []).append((name, line, literal))
    return {stem: tuple(found) for stem, found in index.items()}


def reducible_sites(
    index: dict[str, list[tuple[str, int]]] | None = None,
) -> dict[str, tuple[int, int]]:
    """`family -> (walk sites, Rust reader sites)` — **the queue, per family**.

    ★★★★★ R2175 — the ONE derivation of *what is left to convert*, so that the
    total this gate prints and the per-family questions asked beside it cannot
    be about different things. Everything that says "queue", "blocked" or
    "reducible" reads this; nothing re-derives it.

    ⚠ A walk site is always a reader — a walk cannot declare a Rust const and
    has no test span — which is why the walk column needs no role filter. That
    is a fact about the population, not an assumption: [`sources`] is
    `tools/demos/` plus the harness.

    ⚠⚠ Families with only assertions or only declarations are ABSENT, and that
    is the point. They are not work; charging them as work is what this
    replaced. What they may still be is UNPINNED — a different question, asked
    by [`unpinned_families`].
    """
    index = scan() if index is None else index
    readers = rust_reader_scan()
    out: dict[str, tuple[int, int]] = {}
    for stem in set(index) | set(readers):
        pair = (len(index.get(stem, ())), len(readers.get(stem, ())))
        if pair != (0, 0):
            out[stem] = pair
    return out


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
    # ★★★★★ R2178 — a stem spelled only as a non-address name is not an address
    # family, so it has no row in the address budget. Carrying it at zero would
    # read as *converted*, which it never was; the third state the ratchet
    # lacked is "never a family", and dropping the row is how it is recorded.
    # See `non_address_stems` for why this cannot hide a converted family: that
    # set requires a LIVE non-address spelling and no address spelling at all.
    other = non_address_stems()
    kept = {
        stem: row
        for stem, row in kept.items()
        if not (stem in other and row == Pinned(0, 0))
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
    # ★★★★★ R2178 — and what the Rust needle declined to count, beside the
    # count it qualifies. Printed every run, zero or not: an exclusion nobody can
    # see is a census measuring comfort (R2160), and this one removed half of
    # what the campaign called BLOCKED.
    excluded = [
        f"{path.relative_to(ROOT)}:{line} .{method}(\"{stem}...\")"
        for path in rust_sources()
        for line, stem, method in non_address_literals(
            path.read_text(encoding="utf-8", errors="replace")
        )
    ]
    print(
        f"painted-addresses: {len(excluded)} Rust literal(s) have an address's "
        "shape and are handed to a non-address API — a key of the owner cache, "
        "which names a state slot rather than a mark — so they are not sites"
        + (f": {excluded}" if excluded else "")
    )
    # ★★★★★ R2144 — and WHAT THOSE SITES ARE, which the totals above cannot
    # say. The ratchet counts a spelled address the same whether a painter
    # composed it or a test asserted it, and the two want opposite treatment:
    # a reader is the debt, an assertion is the only value pin a family that
    # still has one has got. Printed every run because the number this debt is
    # judged by has a FLOOR it must not cross, and a floor nobody states is a
    # target nobody can reach.
    reader, assertion, declaration = rust_role_totals()
    # ★★★★★ R2175 — the queue is summed from `reducible_sites`, the per-family
    # derivation everything else about the queue reads, rather than from the
    # grand total beside it. The two must agree, and `--selftest` compares them:
    # before this round the total and the per-family questions came from two
    # walks over the corpus that nothing joined, which is how `blocked_families`
    # could report a number that was not part of the queue it qualified.
    left = reducible_sites(index)
    queue = sum(sum(pair) for pair in left.values())
    print(
        f"painted-addresses: of the rust half, {reader} reader(s), "
        f"{assertion} assertion(s) and {declaration} declaration(s) — the "
        f"reducible queue is {queue} (walks are all readers), and "
        f"the census cannot legitimately fall below {assertion + declaration}"
    )
    # ★★★★★ R2181 — and the retyping the queue CANNOT count: a reader whose
    # address has a runtime value where its family or head goes. R2180 wrote
    # that the queue is a floor under the retyping and could not say by how
    # much; this line says, every run, beside the number it qualifies. Split by
    # whether the site can compose an address a `.pin` artifact holds, because
    # the shape over-counts (an introspection path has it too) and that half is
    # certainly paint. See [`unanchored_reader_sites`].
    loose = unanchored_reader_sites()
    held = sum(
        1
        for _corpus, _where, literal in loose
        if any(template_denotes(literal, address) for address in pinned_addresses())
    )
    walk_loose = sum(1 for corpus, _where, _literal in loose if corpus == "walk")
    # ★★★★★ R2184 — narrowed to the HANDED prefix. A runtime value in the family
    # segment is a template family now and is in the queue above.
    print(
        f"painted-addresses: and {len(loose)} reader site(s) the queue does NOT "
        f"count (walk {walk_loose} / rust {len(loose) - walk_loose}) spell an "
        f"address after a HANDED prefix, whose family no text names — {held} of "
        "them can compose an address a `.pin` artifact holds, so the queue is a "
        "floor under the retyping by at least that much; telling the rest from "
        "file names needs each site's namespace"
    )
    # ★★★★★ R2147 — and WHETHER EACH FAMILY'S VALUE IS HELD AT ALL, which
    # neither line above can say. Converting a family's last speller removes
    # the only comparison its address had unless an artifact or an assertion
    # pins it, so the campaign's own progress can DELETE checks — and until
    # this line nothing counted them.
    pins = pin_sources(families)
    by_artifact = sum(1 for source in pins.values() if source == "artifact")
    by_schema = sum(1 for source in pins.values() if source == "schema")
    by_assertion = sum(1 for source in pins.values() if source == "assertion")
    gone_checks = deleted_checks(now, rust_now, families)
    print(
        f"painted-addresses: {by_artifact} family/ies pinned by an artifact, "
        f"{by_schema} by a declared introspection path, "
        f"{by_assertion} by an assertion, "
        f"{len(families) - by_artifact - by_schema - by_assertion} by NOTHING "
        f"— of those, {len(gone_checks)} are already converted, which is a "
        f"check deleted rather than a debt repaid"
    )
    # ★★★★★ R2167 — WHICH VOCABULARY each remaining site is in. The queue is a
    # work order, and until this line it said one number for three different
    # jobs: a painted address a walk can ask the frame for, a `$schema` path it
    # asks the screen for, and a name NOTHING declares — which is not a
    # conversion at all until something does. Derived from the declarations;
    # see [`site_vocabulary`]. Printed every run, because a queue that cannot
    # say what kind of work it holds is how three rounds picked by eye.
    #
    # ★★★★★ R2175 — over the QUEUE, which is what the paragraph above says it
    # qualifies. The rust column counted every site until this round, so a line
    # whose own sentence is *the queue is a work order* answered about 365 sites
    # when the queue held 25 — and its `unknown` figure, the one a round picks
    # by, was 67 where the truth is what the reducible half spells.
    kinds = ("paint", "path", "both", "unknown")
    walk_kinds = dict.fromkeys(kinds, 0)
    rust_kinds = dict.fromkeys(kinds, 0)
    for stem, (walk_n, rust_n) in left.items():
        walk_kinds[site_vocabulary(stem)] += walk_n
        rust_kinds[site_vocabulary(stem)] += rust_n
    print(
        "painted-addresses: by vocabulary, over the QUEUE — walk "
        + " / ".join(f"{walk_kinds[k]} {k}" for k in kinds)
        + "; rust readers "
        + " / ".join(str(rust_kinds[k]) for k in kinds)
        + " (an UNDECLARED name is not a conversion until something declares "
        "it; BOTH is a name two vocabularies claim, which is a finding)"
    )
    # ★★★★★ R2168 — and HOW MANY OF THE RUST READERS ARE ACTUALLY RETYPED. The
    # debt's own name says `retyped`; this census counts spellings, and the two
    # differ exactly where it matters. See [`rust_reader_duplication`].
    #
    # ★★★★★ R2179 — counted in ADDRESSES (a template can name a concrete
    # member), and the sentence no longer claims a single reader is beyond
    # conversion: whether a declaring home exists was never measured, and three
    # of the single readers at R2179 re-compose a grammar a crate already owns.
    retyped, single = rust_reader_duplication()
    print(
        f"painted-addresses: of the {retyped + single} Rust reader(s), "
        f"{retyped} name an address some other site can also name — the "
        f"RETYPING this debt is named for — and {single} are the only site "
        "naming their address. A single reader is not thereby irreducible: it "
        "goes when something that owns the grammar declares it"
    )
    contested = sorted(
        stem
        for stem in set(now) | set(rust_now)
        if site_vocabulary(stem) == "both"
    )
    if contested:
        print(
            f"painted-addresses: {len(contested)} name(s) claimed by both a "
            f"paint declaration and a `$schema` one: {contested} — a path lives "
            "under an External and a tag lives in the scene, so they do not "
            "collide where they are USED; what cannot tell them apart is this "
            "census reading source literals with no namespace around them"
        )

    # ★★★★★ R2156 — the SECOND vocabulary, whose walk half had no gate at all.
    # Printed every run, zero or not: the number whose absence let this go
    # unseen is the one this line carries.
    keys = spelled_config_keys()
    print(
        f"painted-addresses: {len(keys)} site(s) spell a sourced configuration "
        f"key, over {len(sources())} walk(s) and "
        f"{len(config_key_consumers())} Rust source(s) that can ask "
        f"(the answer here is zero, not a budget)"
    )
    if keys:
        bad = True
        print(
            "painted-addresses: a reader spells a configuration key it could "
            "ask for",
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
            "            spell the key. In RUST, name the const the screen's\n"
            "            own `key.rs` declares.",
            file=sys.stderr,
        )
        return 1
    # ★★★★★ R2160 — how much of the queue above is BLOCKED. Printed every run
    # beside the queue it qualifies, because a remainder and the part of it
    # nobody may touch are two different facts and the first alone reads as a
    # plan. See `blocked_families` for why this is not subtracted.
    #
    # ★★★★★ R2175 — and it is drawn from `reducible_sites` now, the SAME
    # derivation the queue total above is printed from, so "of those site(s)"
    # is true of the sentence rather than merely plausible in it. It said 23
    # until this round and the answer was 4.
    blocked = blocked_families(index)
    blocked_sites = sum(walk + rust for _stem, walk, rust in blocked)
    print(
        f"painted-addresses: {blocked_sites} of those site(s) are BLOCKED, in "
        f"{len(blocked)} family/ies nothing pins — `--owed` names them, and a "
        "conversion that emptied one would be refused as a deleted check"
    )
    # ★★★★★ R2175 — and the WIDER question the line above used to answer by
    # accident: a family whose value nothing holds at all, reducible or not. A
    # declaration nobody compares with the paint is exactly as renameable as a
    # reader nobody compares with the paint; what differs is the remedy, which
    # is why these are two lines and not one number. Reported, never subtracted.
    unpinned = unpinned_families(index, rust_index)
    unpinned_sites = sum(walk + rust for _stem, walk, rust in unpinned)
    print(
        f"painted-addresses: separately, {unpinned_sites} site(s) in "
        f"{len(unpinned)} family/ies have their value held by NOTHING — "
        f"{unpinned_sites - blocked_sites} of them are assertions or "
        "declarations, which are not work here but are renameable with nothing "
        "refusing (see `debt-a-published-address-can-be-renamed-and-nothing-"
        "refuses`)"
    )
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


def pin_word(source: str | None, has_work: bool) -> str:
    """The work order's `pin` column for one family — PURE, so it is testable.

    ★★★★★ R2175 — `BLOCKED` is this file's word for *work no round may finish*,
    and the work order was printing it for every family nothing pins. Nine rows
    said BLOCKED under a summary line that said four: one word carrying two
    facts inside one table, which is this project's most-repeated defect and
    reads as *the campaign is more stuck than it is*.

    A family nothing pins and nothing owes is `unpinned` — its value is held by
    nothing, which is a real finding and a different one, counted by
    [`unpinned_families`] and owned by
    `debt-a-published-address-can-be-renamed-and-nothing-refuses`.

    ⚠ Its own function rather than three lines in the loop, for R1884's reason:
    a rule and the thing that runs it are two gates, and the one that gets
    tested is the pure one.
    """
    if source:
        return source
    return "BLOCKED" if has_work else "unpinned"


def owed() -> int:
    """The work order: the families with the most sites left to convert.

    ★★★★★ R2116 — BOTH columns, and the sort is on their sum. Until this round
    the answer was the walks' remainder alone, and three consecutive rounds
    opened by reconciling that number with a hand count of the Rust side that
    the tool could not produce. A work order that names a third of the work is
    not a work order.

    ★★★★★ R2175 — **and the sort was on the wrong population.** "Left to
    convert" is walk sites plus Rust READERS; the `rust` column is every site,
    so families of pure assertions and pure declarations were ranked as work and
    ranked ABOVE families that had some. Measured at R2175: **52 of the 92
    families here have nothing left to convert**, and the old sort put
    `value.elem` (22 sites, 0 work) at rank THREE and `shell.carry` (9 sites, 0
    work) at rank TEN. The second is the sharper one — R2171 *repaid*
    `shell.carry`, and the work order went on listing it as the tenth-biggest
    job. The `rdr` column is what a round picks by; `rust` stays because it is
    what the ratchet pins and what the floor is computed from.
    """
    index, rust_index = scan(), rust_scan()
    left = reducible_sites(index)
    stems = sorted(
        set(index) | set(rust_index),
        key=lambda s: (
            -sum(left.get(s, (0, 0))),
            -(len(index.get(s, [])) + len(rust_index.get(s, []))),
            s,
        ),
    )
    if not stems:
        print("painted-addresses: nothing spelled anywhere")
        return 0
    pins = pin_sources(stems)
    print(
        f"{'walk':>6}  {'rust':>6}  {'rdr':>5}  {'files':>5}  {'pin':<9}  family"
    )
    for stem in stems:
        walk, rust = index.get(stem, []), rust_index.get(stem, [])
        files = len({path for path, _ in walk} | {path for path, _ in rust})
        # ★★★★★ R2160 — the PIN column, and it is a work order's most important
        # one. A family nothing pins cannot be converted: taking its last
        # speller leaves its address compared with nothing, which `--check`
        # refuses as a deleted check. A round picking the top row of this table
        # used to get no warning at all.
        readers = left.get(stem, (0, 0))[1]
        held = pin_word(pins.get(stem), bool(walk) or bool(readers))
        print(
            f"{len(walk):>6}  {len(rust):>6}  {readers:>5}  {files:>5}  "
            f"{held:<9}  {stem}"
        )
    walk_total = sum(len(f) for f in index.values())
    rust_total = sum(len(f) for f in rust_index.values())
    left_total = sum(sum(pair) for pair in left.values())
    print(
        f"{walk_total:>6}  {rust_total:>6}  {left_total - walk_total:>5}"
        f"         total across {len(stems)} family/ies; {left_total} left to "
        "convert"
    )
    blocked = blocked_families(index)
    sites = sum(walk + rust for _stem, walk, rust in blocked)
    print(
        f"painted-addresses: {len(blocked)} of those family/ies are BLOCKED — "
        f"{sites} site(s) whose value nothing holds, so converting one deletes "
        "a check rather than repaying a debt. Pin them first, or decide they "
        "are not work."
    )
    unpinned = unpinned_families(index, rust_index)
    print(
        f"painted-addresses: a further {len(unpinned) - len(blocked)} family/ies "
        "read `unpinned` — nothing holds their value either, and they have "
        "nothing left to convert, so a rename there is unchecked but it is not "
        "this debt's work"
    )
    return 0


def blocked_families(
    index: dict[str, list[tuple[str, int]]] | None = None,
    rust_index: dict[str, list[tuple[str, int]]] | None = None,
) -> list[tuple[str, int, int]]:
    """`(family, walk, rust readers)` for every family that HAS READERS and NO
    pin — the part of the **reducible queue** no round may finish.

    ★★★★★ R2175 — **this said READERS and filtered on SITES**, and the two
    stopped agreeing the moment a screen put its addresses in a `const` array.
    Measured at R2175: of the 23 sites this reported as BLOCKED, **19 were
    declarations** and five of the nine families had no reducible site at all —
    `hello_modal_refocus.hover` (6), `hello_modal_handoff.hover` (5),
    `hello_window_refocus.hover` (2), `the_tide.vn` (2) and `lab.wire` (1) are
    each a single declaring site per address, which is the state this campaign
    exists to reach. The real figure is **4 sites in 4 families**, so the number
    beside the queue overstated the blocked work by 5.75x.
    ⇒ it reads [`reducible_sites`] now, which is the same derivation the queue
    total is printed from. The wider question — *whose value does nothing hold,
    reducible or not* — is [`unpinned_families`], because it is a different
    question and deserves to be asked rather than folded in.

    ★★★★★ R2160 — **the queue was two populations reported as one.** The
    campaign's closing criterion is *reducible = 0*, and [`check`] prints that
    queue as a single number. Measured at R2160, **81 of its 460 sites** sit in
    families nothing pins: an artifact does not carry them, no assertion holds
    them, and [`deleted_checks`] would refuse the conversion that emptied them.
    So a round taking the top row of [`owed`] could pick work it is not allowed
    to finish, and three rounds running discovered that one family at a time by
    reaching it.
    ⚠ BLOCKED is not the same as *not work*. It says one thing only: a pin has
    to exist before the readers can go. For some families that is a test
    somebody has not written; for others the screen paints the mark in a state
    nothing drives; and for at least two — `lab.advanced` and `lab.scenario`,
    measured at R2159 and R2160 — the shipped document gives the feature
    nothing to draw at all, so no state reaches them and the pin cannot be made
    without a decision about what the demo shows. Those three are different
    problems wearing one word, and telling them apart needs the screen driven.
    ⚠⚠ It is deliberately NOT subtracted from the reducible queue. The queue is
    what the debt's closing criterion reads, and quietly shrinking it by the
    part that is hard would be the census measuring the campaign's comfort
    rather than its remainder — R2155's finding, which this is the second
    instance of.
    """
    left = reducible_sites(index)
    pins = pin_sources(sorted(left))
    return [
        (stem, walk, rust) for stem, (walk, rust) in sorted(left.items())
        if not pins[stem]
    ]


def unpinned_families(
    index: dict[str, list[tuple[str, int]]] | None = None,
    rust_index: dict[str, list[tuple[str, int]]] | None = None,
) -> list[tuple[str, int, int]]:
    """`(family, walk, rust sites)` for every family with ANY site and NO pin.

    ★★★★★ R2175 — the question [`blocked_families`] used to answer by accident,
    given its own name. *Is this family's value held by anything?* is asked of
    every site, because a declaration nobody compares with the paint is exactly
    as renameable as a reader nobody compares with the paint — R2137.3 measured
    that a consistent rename passes 790 tests. What differs is the REMEDY: a
    reader is converted, a declaration is pinned. One word could not carry both.

    ⚠ Reported, never subtracted — R2160's rule, which the round that narrowed
    `blocked_families` is the likeliest to break. The narrower number is the
    honest one for *what is left to convert*; this is the honest one for *what a
    rename would go unnoticed in*, and dropping it to make the first look better
    would be the census measuring comfort.

    ⚠⚠ The repair for the declaration-only half is not this debt's. It is
    `debt-a-published-address-can-be-renamed-and-nothing-refuses`, and this line
    is where that debt's population can be counted from the address census
    rather than by hand.
    """
    index = scan() if index is None else index
    rust_index = rust_scan() if rust_index is None else rust_index
    stems = sorted(set(index) | set(rust_index))
    pins = pin_sources(stems)
    return [
        (stem, len(index.get(stem, [])), len(rust_index.get(stem, [])))
        for stem in stems
        if not pins[stem] and (index.get(stem) or rust_index.get(stem))
    ]


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
    if "{}" in stem:
        # ★★★★★ R2184 — a TEMPLATE family is spelled as a format template,
        # `card.{id}.x`, so its `{}` is matched as a placeholder. By text alone
        # this answered "spelled by no Rust source" for five of the twelve
        # template families whose templates are there, and "yes" for the other
        # seven only where `card.{}` happened to occur as literal text.
        pattern = re.compile(stem_pattern(stem, r"\{[^{}]*\}"))
        if not pattern.search(text):
            return (False, False)
        return (True, any(pattern.search(lit) for lit in RUST_LITERAL.findall(text)))
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
#: ★★★★★ R2163 — **where the artifact lives, and how it is cut, belong to
#: [`painted_grammar`] now and not to this file.** Two readers of one artifact
#: each wrote the cut rule, R2154 measured one of them wrong and fixed it there,
#: and this one kept the broken form for eight rounds. See that module's header.
GRAMMAR_ARTIFACTS = painted_grammar.ARTIFACTS


@functools.lru_cache(maxsize=1)
def pin_artifact_addresses() -> tuple[str, ...]:
    """Every address the committed `.pin` artifacts hold, fold markers removed.

    `repeating_site` folds an index into `#*` so a pin is stable against fixture
    data; the marker is dropped here because this asks which FAMILY is covered,
    and a family is the same one whichever row of it was folded.
    """
    return tuple(line.replace("#*", "") for line in pin_artifact_lines()) + tuple(
        config_surface_paths()
    )


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


def grammar_parts() -> tuple[str, ...]:
    """Every PART WORD a committed grammar artifact can put after a prefix.

    ★★★★★ R2163 — **this used to return whole fixed RUNS and its one caller
    compared them to a family STEM.** A stem is an address's first two segments,
    so `{prefix}.grid.minor.x.{index}` offered `grid.minor.x` to a question
    asking about `chart.grid`, and the two could only ever agree when a run
    happened to be one segment long. Every deep grammar, and every overlay
    template (whose first segment is `{part}`), was invisible to the pin it
    genuinely provides — measured: ten families, 98 walk and 55 Rust sites,
    `chart.inspect` the largest family in the whole queue, and `scatter.inspect`
    reported BLOCKED, which is this census's word for work no round may finish.

    ⇒ the unit is now the one the caller asks in: the single word that can
    follow a prefix. [`painted_grammar.family_heads`] derives it, including the
    overlay parts the artifact itself declares and excluding the caller-supplied
    ones it does not — see there for why that exclusion is the safe direction.
    """
    return tuple(sorted(painted_grammar.family_heads()))


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

    ⚠⚠ R2163 MEASURED A NAMED GAP HERE, and it is the other half of the pin the
    round repaired. This reads `with_tag_prefix` CALLS, so a prefix a chart kind
    carries as its own DEFAULT is invisible: `Timeline` defaults to `timeline`
    and `Sparkline` to `spark`, neither is ever passed to `with_tag_prefix`, and
    the artifact publishes only the crate-wide `DEFAULT_PREFIX`. So
    `timeline.playhead` and `timeline.axis` read as pinned by an assertion while
    the same grammar under `chart` reads as pinned by the artifact. Two kinds,
    a handful of sites — and the repair is not a wider regex here: it is for the
    artifact to PUBLISH the prefix each kind defaults to, the way R2153 made a
    running chart publish the one it took. Registered, not patched.
    """
    found: set[str] = set()
    for path in painted_grammar.artifacts():
        default = painted_grammar.table(path).get("const", {}).get("DEFAULT_PREFIX")
        if default:
            found.add(default.strip())
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


#: How a screen DECLARES an introspection path — the third address vocabulary.
#:
#: ★★★★★ R2166 — measured: a large part of what this census still counted is
#: not a painted address at all. `gq(tf, "value.elem.1")` and
#: `q(tf, "item.dark.checked")` are §7 `$schema` PATHS, the vocabulary R1637-
#: R1642 made a precondition of dispatch (`InterveneError::UnknownPath` is
#: literally "path is not declared in the schema"). [`ADDRESS`]'s needle cannot
#: tell three dotted words apart, so they were counted as paint — the same
#: shape R2155 found for configuration keys, one vocabulary further out.
#:
#: The repair is R2155's: not to drop them, but to recognise the place they are
#: DECLARED, so converting one is a repayment rather than a deleted check.
SCHEMA_DECLARATION = re.compile(r'SchemaField::(?:new|parametric|action)\(\s*"([^"]+)"')


def declarations_outside_tests(text: str) -> list[str]:
    """The paths Rust `text` declares with `SchemaField::…` OUTSIDE every
    `#[cfg(test)]` item — PURE, so the discrimination is a fixture."""
    spans = test_spans(text)
    found: list[str] = []
    for match in SCHEMA_DECLARATION.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        if not any(first <= line <= last for first, last in spans):
            found.append(match.group(1))
    return found


@functools.lru_cache(maxsize=1)
def shipped_declarations() -> tuple[str, ...]:
    """Every introspection path a `SchemaField` declares in code that SHIPS —
    **the one reader of the declarations** [`schema_heads`] and
    [`declared_path_templates`] both ask.

    ★★★★★ R2191 — both read test code as if a screen had declared it, each in
    its own copy of this corpus loop. Measured: 184 parametric declaration
    sites, 26 of them inside a `#[cfg(test)]` item or a [`test_only_modules`]
    file, and three templates declared ONLY there — `a.<x><y>` (a unit test's
    undelimitable path), `cell.<row>` and `hit.<x>` — which also gave the head
    `a`. What that charged: exactly one walk site,
    `r1525_paints_its_model.py:203`, which queries `cell.0` to assert the screen
    answers `UnknownIntrospectPath` — a deliberate probe of a path the screen
    does NOT declare (it ships `cell.<row>.<col>`), billed as a retyping because
    a `pinion-core` unit test declares `cell.<row>`. Found when the two-segment
    rule measured next made a fixture charge `a.{}` through `a.<x><y>`.

    A dry run with every cache cleared: templates 135 -> 132, heads 87 -> 86,
    queue (1268, 32) -> (1267, 32), `cell.{}` walk 7 -> 6; roles, retyped, the
    handed-prefix remainder, BLOCKED, the unpinned families, the deleted check,
    every pin source and every vocabulary unchanged.
    """
    only_test = test_only_modules()
    found: list[str] = []
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            if path in only_test:
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if "SchemaField::" in text:
                found.extend(declarations_outside_tests(text))
    return tuple(found)


@functools.lru_cache(maxsize=1)
def schema_heads() -> frozenset[str]:
    """The fixed head of every PARAMETRIC declared introspection path.

    `expanded.<branch_id>` contributes `expanded`; `any_modified` contributes
    NOTHING, because a path with no argument is one whole path rather than a
    family, and folding it in would claim a family from a declaration that
    names a single slot.

    ⚠⚠ The parametric restriction is what keeps this SAFE, and it was measured
    rather than assumed. A bare head match over ALL declarations would claim
    `line.series` — a painted chart family — from a declaration that has never
    heard of it, and calling a family pinned when it is not loses a check
    silently ([`covers`]). Restricted to parametric heads, the claim is 14
    families, every one of them a path vocabulary the walks read by `query` or
    `intervene`, and not one of them painted.
    """
    heads: set[str] = set()
    for declared in shipped_declarations():
        segments = declared.split(".")
        fixed: list[str] = []
        for segment in segments:
            if segment.startswith("<"):
                break
            fixed.append(segment)
        if fixed and len(fixed) < len(segments):
            heads.add(fixed[0])
    return frozenset(heads)


def schema_pins(stem: str) -> bool:
    """Whether a screen's DECLARED introspection paths hold `stem`'s family.

    This is a value pin in the same sense the emitted grammar is: the
    declaration is what the walk now composes from, and it is checked — the
    router refuses a path the schema does not declare, so a declaration and the
    behaviour it describes cannot drift apart silently.
    """
    head, _, rest = stem.partition(".")
    return bool(rest) and head in schema_heads()


@functools.lru_cache(maxsize=1)
def address_literal_sites() -> tuple[tuple[str, str, str], ...]:
    """`(where, family stem, literal)` for every whole address literal in this
    tree — the population a reader is retyped AGAINST.

    ★★★★★ R2179 — this was `address_spellings`, a count keyed by literal TEXT,
    and text is not the unit a retyping is asked in: a template like
    `feed.row.{n}` can never be textually equal to the concrete `feed.row.3`, so
    every template reader looked like the only place its address was written.
    It now lists SITES, and [`rust_reader_duplication`] asks
    [`may_denote_same`] of them. The corpus rules below are unchanged.

    ★★★★★ R2168 — **this debt is named `a paint address is RETYPED at every
    reader`, and the census counts SPELLINGS.** The two differ exactly where it
    matters: an address written in ONE place is not retyped, and several of the
    ones in the reducible queue are literally the declaration this campaign
    asks readers to call — `fn cell_tag(r, c)` in `hello-deviation-grid` has
    three consumers and a test pinning its value, and its one `format!` is
    counted as a site owing conversion.

    ⚠ Counted across BOTH corpora, deliberately. A Rust reader and a walk that
    spell the same address are two places, so the pair is a retyping across the
    language wall even though each side writes it once. Counting per package
    would have called the shell's reading of the lab's address a single
    spelling, which it is not.

    ⚠⚠ The wide Rust corpus, declaring files INCLUDED: a declaration in
    `address.rs` plus a reader elsewhere is two spellings, and the reader is the
    one that is retyping. Excluding the declaring file would hide that.
    """
    # ★★★★★ R2178 — through THE needle. This was the fourth walk over the Rust
    # corpus with its own copy of the literal loop and the comment rule, so a
    # population rule added to the needle (a key handed to the owner cache is a
    # slot name, not an address) would have gone on being counted here as an
    # address spelling. The corpus stays the WIDE one, declaring files included,
    # for the reason above; only the definition of a spelling is now shared.
    found: list[tuple[str, str, str]] = []
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            where = str(path.relative_to(ROOT))
            for line, stem, literal in rust_site_literals(text):
                found.append((f"{where}:{line}", stem, literal))
    for path in sources():
        where = str(path.relative_to(ROOT))
        for line, literal in _walk_literals(path):
            found.append((f"{where}:{line}", family_of(literal), literal))
    return tuple(found)


#: A placeholder inside an address literal — `{n}`, `{index}`, `{lane}`, `{}`.
_PLACEHOLDER = re.compile(r"\{[^{}]*\}")

#: The two wildcard tokens a placeholder becomes: exactly one character, then
#: any number more. A placeholder fills at least one character of ONE address
#: segment and never crosses a `.` — which is the unit an address is built in.
_ANY, _ANYSTAR = object(), object()


def _segment_tokens(segment: str) -> list:
    """One address segment as literal characters and wildcard tokens."""
    tokens: list = []
    for index, piece in enumerate(segment.split("\x00")):
        if index:
            tokens += [_ANY, _ANYSTAR]
        tokens += list(piece)
    return tokens


def _segments_meet(left: list, right: list) -> bool:
    """Whether two segment patterns can match one common string — exactly."""

    @functools.lru_cache(maxsize=None)
    def meet(i: int, j: int) -> bool:
        if i == len(left) and j == len(right):
            return True
        if i < len(left) and left[i] is _ANYSTAR:
            if meet(i + 1, j):
                return True
            return j < len(right) and right[j] is not _ANYSTAR and meet(i, j + 1)
        if j < len(right) and right[j] is _ANYSTAR:
            if meet(i, j + 1):
                return True
            return i < len(left) and meet(i + 1, j)
        if i == len(left) or j == len(right):
            return False
        a, b = left[i], right[j]
        if a is _ANY or b is _ANY or a == b:
            return meet(i + 1, j + 1)
        return False

    return meet(0, 0)


def may_denote_same(a: str, b: str) -> bool:
    """Whether two address literals can name the same painted address.

    ★★★★★ R2179 — **the unit `retyped` is asked in.** Until this round the
    duplication split compared literal TEXT, so a format template like
    `feed.row.{n}` could never equal the concrete `feed.row.3` a test reads,
    and every template reader was reported as the only place its address is
    written — by construction of the measure, not by any fact about the tree.
    Measured at R2179: the tool said 12 retyped / 8 single, and six of the
    eight were templates. Counted in addresses, it is 15 / 5, and each of the
    three that moved has named sites behind it (`records.row.0` and
    `records.row.7` in its screen's tests; `lab.scenario.main.2` in a test;
    `feed.row.{index}` in the crate that paints the feed).

    ⚠ NOT "the same family", which was measured too and rejected: 18 / 2 under
    that rule, because `feed.row.{n}` and `feed.row.{n}.cell.{k}` share a family
    and sit side by side in one builder, and one does not retype the other.

    EXACT, not a shortcut. Both literals are split into segments at `.`; a
    placeholder fills one or more characters of one segment and never a dot;
    two literals meet iff they have the same number of segments and every pair
    of segments can match a common string. A concrete literal is a template
    with no placeholders, so there is no special case — and a placeholder
    beside literal text in one segment (`col#{n}`, `{from}-{to}`, both in this
    tree) is decided exactly rather than approximated.
    """
    left = _PLACEHOLDER.sub("\x00", a).split(".")
    right = _PLACEHOLDER.sub("\x00", b).split(".")
    if len(left) != len(right):
        return False
    return all(
        _segments_meet(_segment_tokens(x), _segment_tokens(y))
        for x, y in zip(left, right)
    )


#: One segment of an address-shaped literal: word characters, an instance key
#: and placeholders — never a space, a slash or an unbalanced brace.
_SEGMENT = re.compile(r"^(?:[A-Za-z0-9_#:-]|\{[^{}]*\})+$")
_FAMILY_WORD = re.compile(r"^[a-z][a-z0-9_]*$")


def stem_pattern(stem: str, placeholder: str) -> str:
    """A regular expression for `stem`'s segments, each `{}` of a template
    family standing for `placeholder`.

    ★★★★★ R2184 — ONE reading of a family stem, so the two questions asked of
    it cannot disagree about where its segments are: does a pinned ADDRESS
    hold it ([`covers`], where `{}` is one segment's characters) and does a
    Rust SOURCE spell it ([`rust_needles`], where `{}` is a format
    placeholder). For a concrete stem the pattern is its escaped text.
    """
    return r"\.".join(
        placeholder
        if segment == "{}"
        else re.escape(segment).replace(r"\{\}", placeholder)
        for segment in stem.split(".")
    )


def template_family(literal: str) -> str | None:
    """The TEMPLATE FAMILY of a literal whose family segment is a runtime
    value — `card.{id}.config` is `card.{}` — or `None`.

    ★★★★★ R2184 — R2181 measured these and printed them beside the queue
    rather than charging them, because `card.{id}.close` composes every card's
    close and a family-keyed ratchet had no row for it. Charging such a site to
    each family it can compose would count one retyping seven times, which is
    the unit error R2179 removed. The unit this tree retypes is the template's
    own vocabulary, and it is readable from the text exactly as a concrete
    stem is: the family word, and the family segment with every placeholder
    normalised to `{}` and an instance key dropped.

    Proven before it entered the tree, by a dry run of this rule through every
    derivation with all nineteen caches cleared: 12 families, the reducible
    queue 160 -> 358, no existing budget row rising or falling, and the
    unpinned derivation still `echo.demo` alone. `card.{}` is held by an
    artifact, nine by a declared parametric schema head, and `float.{}` and
    `torn.{}` by nothing — as `float.packet` and `torn.packet` already were.

    ⚠ A literal whose HEAD is the placeholder is not one: its family is the
    handed prefix's, which no text names — see [`unanchored_shape`].
    """
    if "{{" in literal or ADDRESS.match(literal):
        return None
    segments = literal.split(".")
    if (
        len(segments) < 3
        or _FAMILY_WORD.match(segments[0]) is None
        or _PLACEHOLDER.search(segments[1]) is None
        or not all(_SEGMENT.match(segment) for segment in segments)
    ):
        return None
    family = re.sub(r"#.*$", "", _PLACEHOLDER.sub("{}", segments[1]))
    return f"{segments[0]}.{family}"


#: A literal carrying an ARGUMENT where a family segment goes — a number
#: (`value.3`, `node.0.x`) or, since R2192, a runtime placeholder (`edge.{e}`,
#: `state.{i}`) — which neither [`ADDRESS`] (a word there) nor, for a
#: two-segment path, [`template_family`] (three segments at least) anchors.
_ARGUMENT_ID = re.compile(
    r"^[a-z][a-z0-9_]*\.(?:\d+|\{[^{}]*\})(?:\.[A-Za-z0-9_#.{}:-]+)?$"
)

#: How a declared path marks where an argument goes: `value.<index>`.
_DECLARED_ARG = re.compile(r"<[A-Za-z_][A-Za-z0-9_]*>")


@functools.lru_cache(maxsize=1)
def declared_path_templates() -> tuple[tuple[str, str], ...]:
    """`(declared path, template)` for every PARAMETRIC path this tree declares —
    `("node.<id>.op", "node.{}.op")` — read with [`SCHEMA_DECLARATION`].

    Over the wide Rust corpus, as [`schema_heads`] reads it: a screen's
    declaration is where the vocabulary lives, whichever crate holds it — and,
    since R2191, only the code that ships ([`shipped_declarations`]).
    """
    found = {d for d in shipped_declarations() if _DECLARED_ARG.search(d)}
    return tuple(sorted((d, _DECLARED_ARG.sub("{}", d)) for d in found))


def declared_path_addresses(declared: str, literal: str) -> bool:
    """Whether the DECLARED path `declared` addresses `literal`, by the rule the
    runtime answers with — `SchemaField::addresses`, step for step.

    ★★★★★ R2195 — each fixed piece of the declaration must lead what is left of
    the literal, and an argument runs to the declaration's NEXT fixed piece (or
    to the end). [`template_denotes`] gives a non-head placeholder exactly one
    segment, which is right for a paint template and wrong for a declared path:
    `menu` declares `checked.<path>` and answers `checked.2.0` (item 0 of menu
    2), because its query reads the whole remainder as the path. The census
    asked the one-segment rule and called 41 walk spellings undeclared —
    `checked.2.0`, `item_kind.2.1`, `item_count.0.2`, `enabled.1.1` — which
    R2189 then registered as screens answering paths they do not declare. Two
    implementations of one matching rule disagreed, and this one was wrong.

    Measured before building: over every shipped parametric declaration, the
    two rules agree on every one-segment argument (0 disagreements), so this is
    a strict widening. A dry run with every cache cleared: walk 1433 -> 1474
    (all 41 in `r805_menu_stateful`, `r832_menu_app` and `r985_menu_nested`);
    the Rust half, roles, retyped, the handed-prefix remainder, BLOCKED, the
    unpinned families and the deleted check unchanged; three new rows
    (`item_kind.{}`, `item_count.{}`, `enabled.{}`), each held by a declared
    schema head; `checked.{}` walk 0 -> 18; none fallen.

    ⚠ Ownership, not validity, as the runtime's own documentation says:
    `node.2.0.op` belongs to `node.<id>.op` with the id `2.0`; whether that id
    is well formed is the surface's answer, not the census's.
    """
    rest, template, first = literal, declared, True
    while "<" in template:
        opening = template.index("<")
        fixed = template[:opening]
        if not rest.startswith(fixed) or (not first and not fixed):
            return False
        rest = rest[len(fixed) :]
        closing = template.find(">", opening)
        if closing < 0:
            return False
        template = template[closing + 1 :]
        after = template.find("<")
        next_fixed = template if after < 0 else template[:after]
        if next_fixed:
            at = rest.find(next_fixed)
            if at < 0:
                return False
            rest = rest[at:]
        else:
            rest = ""
        first = False
    return rest == template


def declared_path_family(
    literal: str, templates: Iterable[tuple[str, str]] | None = None
) -> str | None:
    """The family of a literal with an ARGUMENT where the id goes — a number, or
    since R2192 a runtime placeholder — that composes a declared path's family:
    `node.0.x` is `node.{}`, `edge.{e}` is `edge.{}`; or `None`.

    ★★★★★ R2187 — the census read the §7 path vocabulary (R2166) through the
    paint address's grammar, and that grammar cannot see a path whose argument
    is written as a NUMBER: `ADDRESS` wants a word in the family segment,
    [`template_family`] wants a placeholder there, and most of these paths are
    two segments long, which that grammar calls a prefix. Measured: **760 walk
    and 545 Rust literals** charged nowhere — `value.3`, `node.0.x`,
    `name.11`, `selected.15` — so R2173's "the path vocabulary's walk half is 0"
    was true only of what the needle could see.

    ⚠ SHAPE cannot separate `value.3` from a dictionary key or a word that
    happens to be dotted. The DECLARATIONS can: a literal is a path spelling
    exactly when it composes a declared parametric template, the test R2181
    built for pins ([`template_denotes`]) asked of `SchemaField` declarations.
    Measured before building: of the 760 walk literals, **728 compose a
    declared path and 32 compose none**; of the 545 Rust, 523 compose one and
    every one is an assertion.

    The family is the path's HEAD as a template family (`value.{}`), the key
    [`template_family`] gives the same vocabulary when it is written with a
    placeholder, so `f"node.{i}.op"` and `"node.0.op"` are one row.

    ⚠ One head, and not by luck: a declared path begins with a WORD, and
    [`template_denotes`] matches that word against the literal's own first
    segment — so every template a literal composes shares the literal's head.
    The first draft refused "several heads", a branch that could not run; it is
    gone rather than kept as a check with no failing path.

    ★★★★★ R2192 — and a RUNTIME argument where the id goes. `f"edge.{e}"`,
    `f"state.{i}"` and `format!("state.{i}")` compose the declared `edge.<id>`
    and `state.<index>`, and no rule charged them: [`template_family`] wants
    three segments and this rule wanted a digit. A three-segment one
    (`node.{i}.op`) is [`template_family`]'s already and never reaches here.
    The Rust readers it charges are examples querying their own widget's
    introspection with a retyped path; they have no Rust door to convert to
    yet, which is the next structure to build rather than a reason to leave
    them uncounted. Built on two rounds that made its measurement true first:
    R2190 separated a function that RETURNS such an address from one that
    reads through it (28 readers, not 29), and R2191 stopped a unit test's
    declaration counting as a screen's (its first implementation here failed a
    fixture through a test's `a.<x><y>`, and was reverted until that was fixed).
    Re-measured on that vocabulary, a dry run with every cache cleared: walk
    +166, Rust +35 — queue (1267, 32) -> (1433, 60); retyped (25, 7) ->
    (53, 7); roles (32, 917, 74) -> (60, 923, 75); the handed-prefix
    remainder, BLOCKED, the unpinned families and the deleted check unchanged;
    eight new budget rows, each held by a declared schema head or an artifact;
    36 risen; none fallen.

    ★★★★★ R2195 — and the literal is matched against the DECLARED path by the
    runtime's rule ([`declared_path_addresses`]), not against its `{}` template
    by the paint grammar's one-segment rule: a menu's `checked.2.0` is a
    spelling of `checked.<path>`.

    `templates` is handed in by a fixture; the tree's are
    [`declared_path_templates`].
    """
    if " " in literal or "/" in literal or not _ARGUMENT_ID.match(literal):
        return None
    pool = declared_path_templates() if templates is None else templates
    if any(declared_path_addresses(declared, literal) for declared, _template in pool):
        return f"{literal.split('.')[0]}.{{}}"
    return None


#: ★★★★★ R2189 — where an External's introspection path begins inside an RPC
#: path, read from the tree's one declaration of it, as `pinion-rpc`'s parser
#: reads it (`split_at_external`, R1890).
WIRE_ADDRESS = ROOT / "crates" / "pinion-core" / "src" / "wire_address.rs"
_SEPARATOR_DECLARATION = re.compile(r'pub const SEPARATOR: &str = "([^"]+)";')

#: What may stand before the separator: nothing, or `/`-led segments — a tag
#: chain, an index chain, a `/window[<id>]` prefix. A sentence is not one.
_MOUNT_HEAD = re.compile(r"^(?:/[^/\s]+)*$")


@functools.lru_cache(maxsize=1)
def mount_separator() -> str:
    """The separator an RPC path mounts an External's introspection path behind
    (`pinion_core::wire_address::SEPARATOR`), read from its declaration.

    ⚠ Refuses rather than guesses: a separator this census spelled for itself
    would be a second copy of the grammar, the defect it exists to count.
    """
    hit = _SEPARATOR_DECLARATION.search(WIRE_ADDRESS.read_text(encoding="utf-8"))
    if hit is None:
        raise SystemExit(
            f"painted-addresses: {WIRE_ADDRESS.relative_to(ROOT)} declares no "
            "`pub const SEPARATOR: &str`; the census reads where a mounted path "
            "begins from there and will not spell it itself."
        )
    return hit.group(1)


def address_part(literal: str, separator: str | None = None) -> str:
    """The part of `literal` an address rule judges: the introspection path
    behind a mount — `/external/node.0.x` is `node.0.x` — or the literal itself.

    ★★★★★ R2189 — every rule here was anchored at the START of a literal, and a
    walk that asks through the RPC wire writes the mount first:
    `tf.query("/external/node.0.x")` spells exactly what `q(tf, "node.0.x")`
    spells, and the census charged the second and not the first. R2188 found it
    by sweeping the node editor's walks after converting fifteen of them: 27
    more spelled the same declared paths behind the mount. Measured with this
    census's own scan before building: **463 walk literals in 28 families
    across 67 files**, and none in Rust, which composes its paths
    (`wire_address::path_at`). A dry run through every derivation with all
    twenty caches cleared: walk 805 -> 1268; the Rust half, its roles, the
    handed-prefix remainder, BLOCKED and the deleted checks unchanged; two new
    budget rows, both held by a declared schema head; 26 rows risen, none fallen.

    The head before the separator must itself be path-shaped ([`_MOUNT_HEAD`]),
    so a sentence that mentions a mounted path stays prose. An unresolved
    `{EXT}/node.0.x` carries no separator and is not read: that errs towards not
    charging, the direction the unanchored count already reports.

    `separator` is handed in by a fixture; the tree's is [`mount_separator`].
    """
    sep = mount_separator() if separator is None else separator
    head, found, tail = literal.partition(sep)
    if found and _MOUNT_HEAD.match(head):
        return tail
    return literal


def family_of(literal: str) -> str | None:
    """The family `literal` is charged to: a concrete stem [`ADDRESS`] anchors,
    a [`template_family`], a [`declared_path_family`], or `None`.

    ★★★★★ R2184 — the ONE place a literal becomes a family. Every needle reads
    it, so a concrete stem and a template stem cannot be derived by two rules.
    ★★★★★ R2187 — and a numeric-id path the screens DECLARE, read last because
    it is the only rule that asks the tree rather than the text.
    ★★★★★ R2189 — every rule judges the [`address_part`], so a path written
    behind its mount is charged as the path it spells.
    """
    literal = address_part(literal)
    hit = ADDRESS.match(literal)
    if hit:
        return hit.group(1)
    return template_family(literal) or declared_path_family(literal)


def unanchored_shape(literal: str) -> bool:
    """Whether `literal` has an address's SHAPE and a family NO text names: a
    placeholder as the whole HEAD — `{tag}.row.{slot}`, a prefix handed in.

    ★★★★★ R2181 — R2174 and R2180 each found a screen's addresses composed in
    places this census could not see, and R2180 wrote down that the reducible
    queue is a floor under the retyping, *by how much unmeasured*. This was the
    reader that measured it: 351 readers, against a queue of 61.

    ★★★★★ R2184 — the other shape R2181 counted here, a runtime value in the
    FAMILY segment (`card.{id}.config`), is a [`template_family`] now and the
    ratchet charges it. What remains is the handed prefix, and it stays
    printed rather than charged for a measured reason: the shape also holds
    file names (`{name}.png` nine times in the walks, `{name}.rs` six times in
    Rust tests), and keying it into the ratchet would write file names into
    `docs/unpinned-families.tsv`. Telling the two apart needs each site's
    NAMESPACE — the reader it is handed to — which this census does not have.
    """
    # R2189 — judged on the same part [`family_of`] judges.
    literal = address_part(literal)
    if ADDRESS.match(literal) or "{{" in literal:
        return False
    segments = literal.split(".")
    if len(segments) < 2 or not all(_SEGMENT.match(segment) for segment in segments):
        return False
    if not re.search(r"[a-z]", _PLACEHOLDER.sub("", literal)):
        return False
    return _PLACEHOLDER.fullmatch(segments[0]) is not None


@functools.lru_cache(maxsize=None)
def _template_pattern(template: str) -> re.Pattern[str]:
    parts: list[str] = []
    for index, segment in enumerate(template.split(".")):
        if index == 0 and _PLACEHOLDER.fullmatch(segment):
            parts.append(r"[^.]+(?:\.[^.]+)*")
        else:
            parts.append(
                r"[^.]+".join(re.escape(piece) for piece in _PLACEHOLDER.split(segment))
            )
    return re.compile(r"\.".join(parts) + r"\Z")


def template_denotes(template: str, address: str) -> bool:
    """Whether `template` can compose the concrete `address`.

    ★★★★★ R2181 — [`may_denote_same`]'s rule, with the one difference the
    unanchored shapes need: a placeholder that is the whole HEAD is a handed
    prefix and fills one or more WHOLE segments. Every other placeholder fills
    one or more characters of one segment, exactly as there — and the selftest
    holds the two to agreement wherever both apply, because they are two
    implementations of one rule.
    """
    return _template_pattern(template).match(address) is not None


@functools.lru_cache(maxsize=1)
def pin_artifact_lines() -> tuple[str, ...]:
    """Every address line of the committed `.pin` artifacts, as written — **the
    one reader of that format**; fold markers are each caller's question."""
    out: list[str] = []
    for path in sorted(ROOT.glob(PIN_ARTIFACTS)):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for line in text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                out.append(line)
    return tuple(out)


@functools.lru_cache(maxsize=1)
def pinned_addresses() -> tuple[str, ...]:
    """Every address a `.pin` artifact holds, each fold standing for ONE value.

    A pin is folded by `pinion_core::containment::repeating_site`, which writes
    `*` where a POSITION was — a run of digits and index separators — in three
    places: a whole segment (`3_2`), a `#` suffix (`#0`), and an index glued to
    a name (`0_direction` → `*_direction`). Every one of them began with a
    digit, so `0` is a member of each, and one replacement is the whole rule.
    ★ The selftest's fold arm is what found the third shape: the first draft
    knew two, measured from the ten commonest fold forms rather than all.

    ⚠ Not [`pin_artifact_addresses`], which DROPS the marker because it asks
    which family is covered. This asks whether a template can compose a held
    address, and dropping the marker changes the address — `card.alarms#*.feed`
    would read as `card.alarms.feed`, which nothing paints.
    """
    return tuple(line.replace("*", "0") for line in pin_artifact_lines())


def rust_unanchored_literals(text: str) -> list[tuple[int, str]]:
    """`(line, literal)` for every Rust literal of [`unanchored_shape`] — the
    same scan and the same owner-cache rule as [`rust_site_literals`]."""
    return sorted(
        (line, literal)
        for offset, line, literal in rust_literals(text)
        if unanchored_shape(literal) and handed_to(text, offset) not in NON_ADDRESS_CALLS
    )


def unanchored_reader_sites() -> tuple[tuple[str, str, str], ...]:
    """`(corpus, where, literal)` for every READER whose address this census
    cannot give a family — the retyping the reducible queue does not count.

    Drawn from the RATCHET's populations, so it sits beside the queue on the
    same terms: every walk site is a reader, and a Rust site is one under
    [`site_role_at`].

    ⚠⚠ REPORTED, NOT CHARGED. R2181 printed both unanchored shapes here; since
    R2184 the literal-headed one (`card.{id}.close`) is a [`template_family`]
    and charged by the ratchet, one row per template rather than one per family
    it can compose. What remains is the handed prefix, whose family no text
    names and whose shape file names share — see [`unanchored_shape`]. It is
    printed every run beside the queue it qualifies.
    """
    found: list[tuple[str, str, str]] = []
    for path in sources():
        where = str(path.relative_to(ROOT))
        for line, literal in _walk_unanchored(path):
            found.append(("walk", f"{where}:{line}", literal))
    for path in rust_sources():
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        where = str(path.relative_to(ROOT))
        role_at = site_role_at(path, body)
        for line, literal in rust_unanchored_literals(body):
            if role_at(line) == "reader":
                found.append(("rust", f"{where}:{line}", literal))
    return tuple(found)


def _walk_literals(path: Path) -> list[tuple[int, str]]:
    """`(line, whole address)` for every address literal a walk spells.

    [`sites`] answers the same population as family STEMS; this keeps the whole
    address, which is what [`address_literal_sites`] lists and
    [`may_denote_same`] compares. Since R2179 an f-string keeps its placeholder
    SHAPE (`{}` per interpolated value), so a walk's composed address can be
    matched against a Rust template rather than truncated to its constant head.
    Since R2189 the address carried is the [`address_part`]: a mounted path is
    compared as the path, not as the RPC string around it.
    """
    return [
        (line, address_part(literal))
        for line, literal in python_literals(path.read_text(encoding="utf-8"))
        if family_of(literal)
    ]


def _walk_unanchored(path: Path) -> list[tuple[int, str]]:
    """`(line, literal)` for every walk literal of [`unanchored_shape`]."""
    return [
        (line, literal)
        for line, literal in python_literals(path.read_text(encoding="utf-8"))
        if unanchored_shape(literal)
    ]


def _module_constants(tree: ast.Module) -> dict[str, str]:
    """`NAME -> value` for every name bound EXACTLY ONCE in the whole file, by a
    module-level assignment of a brace-free string.

    ★★★★★ R2181 — the walk half of [`rust_str_constants`]. `VIEW = "nodeflow"`
    and then `f"{VIEW}.node.{n}.label"` spells `nodeflow.node.{n}.label`, and the
    census read the interpolation as a runtime value. Measured before writing:
    38 walk literals anchor once such a name is substituted.

    ⚠ ONCE IN THE FILE, not once at module level: a parameter, a loop variable,
    an import, a `global` or any other binding of the same name anywhere makes
    which value an f-string captures a scoping question, and this answers it by
    not substituting. That errs towards NOT anchoring, which the unanchored
    count still sees.
    """
    bindings: dict[str, int] = {}

    def bind(name: str, times: int = 1) -> None:
        bindings[name] = bindings.get(name, 0) + times

    matched = tuple(
        getattr(ast, kind) for kind in ("MatchAs", "MatchStar") if hasattr(ast, kind)
    )
    for node in ast.walk(tree):
        if isinstance(node, ast.Name) and not isinstance(node.ctx, ast.Load):
            bind(node.id)
        elif isinstance(node, ast.arg):
            bind(node.arg)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            bind(node.name)
        elif isinstance(node, ast.alias):
            bind((node.asname or node.name).split(".")[0])
        elif isinstance(node, (ast.Global, ast.Nonlocal)):
            for name in node.names:
                bind(name, 2)
        elif isinstance(node, ast.ExceptHandler) and node.name:
            bind(node.name)
        elif matched and isinstance(node, matched) and node.name:
            bind(node.name)
    found: dict[str, str] = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            target, value = node.targets[0], node.value
        elif isinstance(node, ast.AnnAssign):
            target, value = node.target, node.value
        else:
            continue
        if (
            isinstance(target, ast.Name)
            and isinstance(value, ast.Constant)
            and isinstance(value.value, str)
            and not {"{", "}"} & set(value.value)
            and bindings.get(target.id) == 1
        ):
            found[target.id] = value.value
    return found


@functools.lru_cache(maxsize=None)
def python_literals(source: str) -> tuple[tuple[int, str], ...]:
    """`(line, literal)` for every string a Python `source` writes — **the walk
    half's one scan**, docstrings and f-string halves excluded (see
    [`_skipped`]).

    ★★★★★ R2181 — keyed by the SOURCE rather than a path, so the three readers
    of one walk share one parse per run and a selftest fixture written to a
    reused temporary name can never be answered from another file's cache.

    ★★★★★ R2179 — an f-string is the WHOLE template, `{}` for every
    interpolated value. It kept only the constant halves, so `f"feed.row.{n}"`
    became `feed.row.` and could never denote the address a Rust template
    composes — the unit error [`may_denote_same`] repairs, one language over.
    Since R2181 an interpolated NAME bound once to a module string is its value
    instead (see [`_module_constants`]).
    """
    try:
        tree = ast.parse(source)
    except SyntaxError:
        # A file this tool cannot parse is a file it must not judge.
        return ()
    skip = _skipped(tree)
    constants = _module_constants(tree)

    def piece(part: ast.expr) -> str:
        if isinstance(part, ast.Constant) and isinstance(part.value, str):
            return part.value
        if (
            isinstance(part, ast.FormattedValue)
            and isinstance(part.value, ast.Name)
            and part.conversion == -1
            and part.format_spec is None
            and part.value.id in constants
        ):
            return constants[part.value.id]
        return "{}"

    found: list[tuple[int, str]] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            if id(node) not in skip:
                found.append((node.lineno, node.value))
        elif isinstance(node, ast.JoinedStr):
            found.append((node.lineno, "".join(piece(part) for part in node.values)))
    return tuple(found)


def rust_reader_duplication() -> tuple[int, int]:
    """`(retyped, single)` over the Rust READERS the reducible queue counts.

    `retyped` is a reader whose address some OTHER site can also name — the
    defect this debt is named for. `single` is a reader no other site denotes.

    ★★★★★ R2179 — **asked in addresses, not in literal text.** A reader is
    retyped iff another site in the same family carries a literal that
    [`may_denote_same`] says can name one of the addresses this one names.
    Counted by text, a template like `feed.row.{n}` could never match the
    concrete `feed.row.3` a test reads, so every template reader was reported
    single: measured at R2179, 12 / 8 by text against 15 / 5 in addresses.

    🟥 **And `single` says only what was measured.** This docstring used to add
    that a single reader "cannot be made to disappear by converting it: there
    is nothing above it to call". Whether a declaring home exists or could
    exist was never measured, and at R2179 three of the five single readers are
    the shell's `spec.rs` re-composing the feed widget's grammar, whose own
    crate already spells two members of it. The inference is gone rather than
    reworded.

    ⚠⚠ REPORTED, NOT SHED. R2160's rule stands — a census that drops its hard
    part measures comfort instead of remainder — so the queue keeps counting
    single readers while this says how many they are.
    """
    # ★ R2178 — derived from `rust_reader_scan`, which carries each reader's
    # literal; the arithmetic arm in `selftest` (retyped + single == the role
    # tally's readers) holds this to an independent walk.
    by_family: dict[str, list[tuple[str, str]]] = {}
    for where, stem, literal in address_literal_sites():
        by_family.setdefault(stem, []).append((where, literal))
    retyped = single = 0
    for stem, sites in rust_reader_scan().items():
        others = by_family.get(stem, [])
        for path, line, literal in sites:
            me = f"{path}:{line}"
            if any(
                where != me and may_denote_same(literal, theirs)
                for where, theirs in others
            ):
                retyped += 1
            else:
                single += 1
    return retyped, single


def site_vocabulary(stem: str) -> str:
    """Which of this tree's address vocabularies `stem` belongs to:
    `"paint"`, `"path"`, `"both"`, or `"unknown"`.

    ★★★★★ R2167 — the debt R2166 registered, paid. That round found that a
    large part of what this census counted is not a painted address at all but
    a §7 `$schema` query path, and it settled WHICH sites by READING them —
    the method this campaign refuses everywhere else. This is the derivation,
    and it asks the two DECLARATIONS rather than the readers:

    * **paint** — a committed `.pin` artifact covers the family, or an emitted
      grammar composes it (see [`grammar_pins`], anchored on both halves).
    * **path** — a parametric `SchemaField::…` declares it (see
      [`schema_pins`], restricted to parametric for the reason recorded there).
    * **unknown** — neither, which is the honest third answer and not a guess.

    ⚠ This is NOT [`pin_sources`] with different words. That answers *what
    holds this family's value* and can say `assertion`; this answers *what kind
    of name it is*, and an assertion holds a value without saying which
    vocabulary it is in. Two questions that happen to share two authorities.

    ⚠⚠ **`"both"` IS AN ANSWER AND NOT A TIE TO BREAK**, and the invariant that
    says so caught one on its first run. `match.<i>` and `match.<row>` are
    declared paths in `row_search` and `row_style`; `match.spark` is a mark
    `hello-analyzer-shell` paints and pins. One word, two vocabularies — this
    tree's recurring defect, found by a gate for once instead of by reading.

    ⚠⚠⚠ And the collision is in THIS NEEDLE, not in the tree: a path lives
    under an External and a tag lives in the scene, so the two never meet where
    they are used. What cannot tell them apart is a census reading source
    literals with no namespace around them. Preferring one here would hide
    that, so the answer is `"both"` and the limit is stated: a tighter
    derivation needs each SITE's namespace — the reader it is handed to — and
    that is the next instalment, not a preference written here.
    """
    paints = grammar_pins(stem) or any(
        covers(address, stem) for address in pin_artifact_addresses()
    )
    routes = schema_pins(stem)
    if paints and routes:
        return "both"
    if paints:
        return "paint"
    if routes:
        return "path"
    return "unknown"


def grammar_pins(stem: str) -> bool:
    """Whether a committed grammar artifact holds `stem`'s family.

    The stem is `<prefix>.<part>` — exactly two segments, which is what
    [`ADDRESS`] captures. Both halves must answer: the part must be a word some
    crate's grammar can put after a prefix, and the prefix must be one a chart
    in this tree is actually given — see [`grammar_prefixes`] for why the second
    half is not optional.

    ⚠ R2163 — the two halves are now asked in the same UNIT. They were not
    before: [`grammar_parts`] offered fixed runs to a two-segment question.
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

    ★★★★★ R2184 — a TEMPLATE family is held the same way, its `{}` segment
    matching one segment of the address: `card.{}` is held by
    `card.alarms.feed`. Written through [`stem_pattern`], and proven against
    the rule it replaced before it did: for every concrete stem in the budget
    against every pinned address — 174 x 1,059, 184,266 pairs — the two
    answered the same every time.
    """
    pattern = r"(?:^|\.)" + stem_pattern(stem, r"[^.]+") + r"\."
    return re.search(pattern, f"{address}.") is not None


@functools.lru_cache(maxsize=1)
def asserted_families() -> dict[str, int]:
    """family stem -> how many ASSERTIONS spell it, over the wide Rust corpus.

    ⚠ R2164 — the role comes from [`rust_site_roles`] now and not from a second
    copy of the rule. The copy this replaced required a literal `#[cfg(test)]`
    INSIDE the file, which a test-only module need not contain, so it answered
    "nothing asserts this" for lines the same tool was counting as assertions
    one function away. See [`rust_site_roles`] for what that was labelling.

    ★ The POPULATION is still the wide one — every Rust file under
    [`RUST_ROOTS`], declaring files included — because this asks whether a
    value is held ANYWHERE, not how much this tree still owes.
    """
    found: dict[str, int] = {}
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for _line, stem, role in rust_site_roles(path, text):
                if role == "assertion":
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
        elif schema_pins(stem):
            # R2166 — the THIRD vocabulary's pin. Named apart from `artifact`
            # because it is a different claim: a declared path is held by the
            # router refusing an undeclared one, not by a byte comparison.
            out[stem] = "schema"
        elif asserted.get(stem):
            out[stem] = "assertion"
        else:
            out[stem] = ""
    return out


@functools.lru_cache(maxsize=1)
def non_address_stems() -> frozenset[str]:
    """Stems the Rust census population spells ONLY as a non-address name —
    a key handed to a [`NON_ADDRESS_CALLS`] method — and never as an address.

    ★★★★★ R2178 — **the budget had two states for a family at zero and needed
    three.** [`write_budget`] carries every family it has ever seen forward, so
    a converted family cannot quietly reacquire a speller, and
    [`deleted_checks`] reads any family at zero with no pin as *a check the
    campaign deleted*. Both are right for an address. Neither has a word for a
    stem that was never one: correcting the needle to stop counting the two
    owner-cache keys would have dropped them to zero with no pin, `--check`
    would have refused that as two deleted checks, and re-pinning would have
    written them into `docs/unpinned-families.tsv` — growing the unreachable
    half of this debt's closing criterion from two false positives to four.

    ⚠ What separates these from a converted family is STATIC, and exact: a
    converted family has no live occurrence left, while these are still spelled
    in live code — only as a slot name. That is not the property R2172 found no
    static test for (`echo.demo` survives only in comments, as converted
    families' doc comments do), and it does not touch that finding.

    ⚠ A stem that is ALSO spelled as an address anywhere in the population is
    not in this set: it is a real family, and its non-address uses stay out of
    its count by the needle rather than by this.
    """
    as_address: set[str] = set()
    as_other: set[str] = set()
    for path in rust_sources():
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        as_address.update(stem for _line, stem, _lit in rust_site_literals(text))
        as_other.update(stem for _line, stem, _method in non_address_literals(text))
    return frozenset(as_other - as_address)


def deleted_checks(
    now: dict[str, int], rust_now: dict[str, int], families: Iterable[str]
) -> list[str]:
    """The families that are CONVERTED and pinned by nothing.

    ★★★★★ A family here is not work that is left — it is a check that is gone.
    Every reader stopped spelling its address, which is the repair, and nothing
    holds the address to a value any more, which is worse than the state before
    the repair: the literal a reader used to carry was at least compared with
    the paint on every run.

    ★★★★★ R2178 — a stem in [`non_address_stems`] is never here. It reached
    zero because the needle stopped calling a slot name an address, not because
    a reader converted, so there was never a check to delete.
    """
    pins = pin_sources(families)
    other = non_address_stems()
    return sorted(
        stem
        for stem in families
        if not now.get(stem, 0)
        and not rust_now.get(stem, 0)
        and not pins[stem]
        and stem not in other
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
    for path in config_key_consumers():
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        name = str(path.relative_to(ROOT))
        found.extend((name, line, key) for line, key in rust_config_keys_in(text))
    return sorted(found)


@functools.lru_cache(maxsize=1)
def config_key_consumers() -> tuple[Path, ...]:
    """Every Rust source that could ASK the option surface instead of spelling it.

    ★★★★★ R2157 — **the population is DERIVED from who compiles the surface in,
    and that boundary is the whole correctness of this gate.**
    [`spelled_config_keys`] used to cover the walks only; the Rust half was a
    per-crate test inside `hello-node-lab`, which is the defect this campaign is
    about one layer up — a second crate would need a second copy, and the copy
    is what goes stale.
    ⚠⚠ **But the needle must NOT be pointed at the whole tree, and this is
    measured rather than assumed.** `pinion-core` and `pinion-widget-paint`
    spell four of these paths **57 times** — `ConfigField::new("listen.endpoints",
    …)` and friends, fixtures for a GENERIC config-form widget. Those crates have
    no option surface, cannot depend on a demo's JSON, and are not re-typing
    anything: they are inventing a plausible key, which is the correct thing for
    a framework test to do. Flagging them would demand the framework depend on a
    consumer, which is backwards.
    ⇒ a package is in this population **iff its own sources reference the surface
    artifact** — iff it can ask. That is derivable, it is one package today, and
    it extends itself the day a second one compiles the surface in. Nothing here
    is a list.
    ⚠ Declaring files are excluded for [`RUST_DECLARING_FILES`]' reason: a
    declaration is supposed to be full of these literals.
    """
    artifact = Path(CONFIG_SURFACE_ARTIFACTS).name
    out: list[Path] = []
    for package in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/")):
        sources_of = sorted(package.glob("src/**/*.rs"))
        reaches = any(
            artifact in path.read_text(encoding="utf-8", errors="replace")
            for path in sources_of
        )
        if reaches:
            out.extend(
                path for path in sources_of if path.name not in RUST_DECLARING_FILES
            )
    return tuple(out)


def rust_config_keys_in(source: str) -> list[tuple[int, str]]:
    """`(line, key)` for every sourced configuration key one RUST source spells.

    [`config_keys_in`]'s counterpart: the needle is the same set of dotted paths
    and only the way a literal is recognised differs, because Rust is not parsed
    here. A line whose first non-space is `//` is prose about a key rather than
    a use of one, which is the same rule [`rust_roles`] applies.
    """
    needles = {path for path in config_surface_paths() if "." in path}
    found: list[tuple[int, str]] = []
    for number, line in enumerate(source.splitlines(), start=1):
        if line.lstrip().startswith("//"):
            continue
        found.extend(
            (number, literal[1:-1])
            for literal in RUST_LITERAL.findall(line)
            if literal[1:-1] in needles
        )
    return found


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
        (
            "★★★★★ R2181 — an instance key belongs to the family segment, and "
            "the stem drops it",
            'a = "card.alarms#6.feed.row.0"\n',
            ["card.alarms"],
        ),
        (
            "★★★★★ and a templated key is the same family",
            'a = f"card.packet#{n}.grip"\n',
            ["card.packet"],
        ),
        (
            "★★★★★ R2181 — a module string interpolated at the head IS its value",
            'VIEW = "nodeflow"\na = f"{VIEW}.node.{n}.label"\n',
            ["nodeflow.node"],
        ),
        (
            "★★ but not when the name is bound anywhere else in the file",
            'VIEW = "nodeflow"\ndef f(VIEW):\n    return f"{VIEW}.node.x"\n',
            [],
        ),
        (
            "★★ nor through a conversion, which changes the text",
            'VIEW = "nodeflow"\na = f"{VIEW!r}.node.x"\n',
            [],
        ),
        (
            "★★★ R2181 — a key spelled after a handed prefix is anchored nowhere; "
            "the constant-halves rule counted this at `listen.endpoints`",
            "a = f\"{parts['item']}listen.endpoints.add\"\n",
            [],
        ),
        (
            "★★★★★ R2184 — a runtime value in the family segment is the TEMPLATE "
            "family, one row for the vocabulary rather than one per card kind",
            'a = f"card.{cid}.grip"\n',
            ["card.{}"],
        ),
        (
            "★★ but a handed prefix still names no family",
            'a = f"{card}.grip.x"\n',
            [],
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
    role_cases: list[tuple[str, str, tuple[int, int, int]]] = [
        (
            "a literal in production is a reader",
            'fn view() { tag("lab.node.T-01"); }\n',
            (1, 0, 0),
        ),
        (
            "a literal inside a #[cfg(test)] module is an assertion",
            '#[cfg(test)]\nmod tests {\n  fn t() { assert("lab.node.T-01"); }\n}\n',
            (0, 1, 0),
        ),
        (
            "★★★★★ a production item AFTER the test module is still a READER — "
            "the naive first-marker rule called this an assertion, and 54 files "
            "of this tree have the shape",
            '#[cfg(test)]\nmod tests {\n  fn t() { assert("lab.node.A"); }\n}\n'
            'const W: &str = "lab.node.B";\n',
            (1, 1, 0),
        ),
        (
            "★ a #[cfg(test)] on a single fn covers only that fn",
            '#[cfg(test)]\nfn helper() { tag("lab.node.A"); }\n'
            'fn view() { tag("lab.node.B"); }\n',
            (1, 1, 0),
        ),
        (
            "a comment is neither a reader nor an assertion",
            '// lab.node.T-01 is the card\nfn view() {}\n',
            (0, 0, 0),
        ),
        (
            "two test modules both count as assertions",
            '#[cfg(test)]\nmod a {\n  fn t() { assert("lab.node.A"); }\n}\n'
            '#[cfg(test)]\nmod b {\n  fn t() { assert("lab.node.B"); }\n}\n',
            (0, 2, 0),
        ),
    ]
    for label, fixture, want in role_cases:
        got = rust_roles(Path("fixture.rs"), fixture)
        if got != want:
            failed += 1
            print(f"FAIL: {label}: rust_roles -> {got}, wanted {want}", file=sys.stderr)

    # ★★★★★ R2169 — the DECLARING SITE, against fixtures for the reason above:
    # what must not rot is the discrimination between a name other code calls
    # and a literal used inline. Tested on [`declaring_lines`] rather than
    # through `rust_roles`, because the "is it used" half needs a package and
    # the rule must be checkable without one.
    used = {
        "HOVER_KEYS",
        "CARD_TAG",
        "SPAN",
        "cell_tag",
        "card_scene",
        "node_path",
        "result_tag",
        "read_kind",
        "read_title",
    }
    declaring_cases: list[tuple[str, str, set[int]]] = [
        (
            "a const bound to one address is a declaration",
            'const CARD_TAG: &str = "lab.node.T-01";\n',
            {1},
        ),
        (
            "★ a multi-line const array covers EVERY line it holds — the shape "
            "that had six addresses charged as six readers",
            'const HOVER_KEYS: [&str; 2] = [\n    "a.hover.one",\n    "a.hover.two",\n];\n',
            {1, 2, 3, 4},
        ),
        (
            "★★ a const whose name NOTHING else calls is dead code, not a "
            "declaration — the half that keeps this from excusing a family "
            "whose address nothing reaches",
            'const UNUSED: &str = "lab.node.T-01";\n',
            set(),
        ),
        (
            "an inline literal is not a declaration",
            'fn view() { tag("lab.node.T-01"); }\n',
            set(),
        ),
        (
            "★ a let binding is not one either — it is not a NAME other code "
            "can call, only a local",
            'fn view() { let t = "lab.node.T-01"; use_it(t); }\n',
            set(),
        ),
        # ★★★★★ R2170 — the other syntactic form of the same declaration.
        (
            "★★ a fn that RETURNS a string is a composer, and its body is the "
            "declaration — the form R2169's const rule stopped short of",
            'fn cell_tag(r: usize) -> String {\n    format!("dev.cell.{r}")\n}\n',
            {1, 2, 3},
        ),
        (
            "★★ and the RETURN TYPE is what says so: a fn that returns a Scene "
            "and happens to tag a node is an ordinary reader, however short",
            'fn card_scene(n: &str) -> Scene {\n    boxed().with_tag("a.node.x")\n}\n',
            set(),
        ),
        (
            "★ a composer nothing else calls is dead code here too",
            'fn unused_tag() -> String {\n    format!("dev.cell.0")\n}\n',
            set(),
        ),
        # ★★★★★ R2190 — a WRAPPED string return, and the call clause that the
        # widening made necessary.
        (
            "★★ a composer may return its string wrapped in an Option",
            'fn node_path(id: u32) -> Option<String> {\n    Some(format!("node.{id}.x"))\n}\n',
            {1, 2, 3},
        ),
        (
            "★ or in a Result",
            'fn result_tag(i: usize) -> Result<String, Error> {\n'
            '    Ok(format!("dev.cell.{i}"))\n}\n',
            {1, 2, 3},
        ),
        (
            "★★ but a fn that HANDS its address to a call reads through it, "
            "whatever it returns — that line is not the declaration",
            'fn read_kind(intro: &I, i: usize) -> Option<String> {\n'
            '    intro.query(&format!("kind.{i}.x")).ok()\n}\n',
            {1, 3},
        ),
        (
            "★ and so does a bare-string fn handing a literal straight to one",
            'fn read_title(intro: &I) -> String {\n'
            '    intro.query("lab.node.title").unwrap()\n}\n',
            {1, 3},
        ),
    ]
    for label, fixture, want in declaring_cases:
        got = declaring_lines(fixture, lambda name: name in used)
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: declaring_lines -> {sorted(got)}, "
                f"wanted {sorted(want)}",
                file=sys.stderr,
            )
    # ★★★★★ R2190 — which call a literal is handed to, looking through the
    # `format!` it is nearly always wrapped in.
    call_argument_cases: list[tuple[str, str, str | None]] = [
        ("a literal handed straight to a method", 'intro.query("a.b.c")', "query"),
        ("★★ and one handed through a borrowed format!",
         'intro.query(&format!("a.b.{i}"))', "query"),
        ("through an unborrowed one", 'node.with_tag(format!("a.b.{i}"))', "with_tag"),
        ("★ a format! that is RETURNED is handed to nothing",
         'Some(format!("a.b.{i}"))', None),
        ("a free function is not a method", 'tag(format!("a.b.{i}"))', None),
    ]
    for label, fixture, want in call_argument_cases:
        got = call_argument(fixture, fixture.index('"'))
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: call_argument({fixture!r}) -> {got!r}, wanted {want!r}",
                file=sys.stderr,
            )

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
        # ★★★★★ R2163 — the cases the eight rounds above could not have caught,
        # because every one of them is a grammar with MORE than one fixed
        # segment, or an overlay whose first segment is `{part}`. The census
        # compared a fixed RUN to a two-segment stem, so these read as pinned by
        # an assertion — and `scatter.inspect` as pinned by NOTHING, which is
        # this file's word for work no round may finish.
        ("★ a DEEP grammar pins its family by its FIRST word", "chart.axis", True),
        ("★ the same, where two templates share that word", "chart.grid", True),
        ("★ and where the segment after it carries the argument", "chart.label", True),
        ("★★ an OVERLAY part is a family the artifact declares", "chart.inspect", True),
        ("★★ under a custom prefix too — 68 sites sat here", "scatter.inspect", True),
        (
            "★★★ but an overlay part is still only pinned under a prefix a\n"
            "       chart takes: this is the half that keeps the widening safe",
            "panel.inspect",
            False,
        ),
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
    # ★★★★★ R2166 — the THIRD vocabulary's pin, and above all the half that
    # says NO. A declared path's head is an ordinary word — `value`, `name`,
    # `expanded`, `item` — so the restriction to PARAMETRIC declarations is
    # what keeps this from claiming a painted family. Measured: without it,
    # `line.series` (a chart family under the `line` prefix) is claimed by a
    # declaration that has never heard of it.
    schema_cases: list[tuple[str, str, bool]] = [
        ("a parametric declaration pins its family", "value.elem", True),
        ("★ the same, under another screen's word", "expanded.struct", True),
        ("★ and one whose argument is not the family's second segment", "item.dark", True),
        (
            "★★ a PAINTED family whose first word a declaration happens to\n"
            "       share is NOT claimed — the direction that loses a check",
            "line.series",
            False,
        ),
        ("a word no schema declares at all", "chart.candle", False),
        ("a bare stem with no second segment", "value", False),
    ]
    for label, stem, want in schema_cases:
        if schema_pins(stem) is not want:
            failed += 1
            print(
                f"FAIL: {label}: schema_pins({stem!r}) -> {schema_pins(stem)}, "
                f"wanted {want}",
                file=sys.stderr,
            )
    if not schema_heads():
        failed += 1
        print(
            "FAIL: no parametric schema declaration was read — every case above "
            "is vacuous",
            file=sys.stderr,
        )
    # ★★★★★ R2191 — a declaration inside a test is a fixture, not a screen's
    # vocabulary; one in shipped code is read wherever it sits beside tests.
    declaration_cases: list[tuple[str, str, list[str]]] = [
        ("a declaration in shipped code is read",
         'const F: &[SchemaField] = &[SchemaField::parametric("row.<i>", "text", A)];\n',
         ["row.<i>"]),
        ("★★ one inside a #[cfg(test)] item is a unit test's fixture",
         'fn view() {}\n#[cfg(test)]\nmod tests {\n'
         '    const F: SchemaField = SchemaField::parametric("a.<x><y>", "text", A);\n}\n',
         []),
        ("★ and shipped code AFTER a test module is still read",
         '#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n'
         'const G: SchemaField = SchemaField::new("count", "int");\n',
         ["count"]),
    ]
    for label, fixture, want in declaration_cases:
        got = declarations_outside_tests(fixture)
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: declarations_outside_tests -> {got!r}, wanted {want!r}",
                file=sys.stderr,
            )

    # ★★★★★ R2167 — the vocabulary derivation, and the invariant that keeps it
    # from HIDING the ambiguity it would otherwise resolve by preference.
    # `site_vocabulary` tests paint first, so a family both authorities claimed
    # would read as paint and nobody would learn that two declarations disagree
    # about one name — this tree's recurring defect. Derived over the live
    # census rather than over examples, so it speaks up the day it happens.
    contested = [
        stem
        for stem in set(census()) | set(rust_census())
        if schema_pins(stem)
        and (grammar_pins(stem) or any(covers(a, stem) for a in pin_artifact_addresses()))
    ]
    mislabelled = [stem for stem in contested if site_vocabulary(stem) != "both"]
    if mislabelled:
        failed += 1
        print(
            f"FAIL: {mislabelled} are claimed by BOTH declarations and this "
            "derivation answered with one of them. A name two vocabularies "
            "claim is a finding, not a tie to break.",
            file=sys.stderr,
        )
    # ★★★★★ R2168 — the RETYPED / SINGLE split, held to the count it splits.
    # Derived, so it cannot drift from the reader total the queue is built on:
    # a bug that lost or invented sites would show up as arithmetic rather than
    # as a number nobody re-derives.
    # ★★★★★ R2170 — the census's Rust TOTAL and the role tally are one
    # population, so they must add up. They did not: the comment rule lived in
    # `rust_site_roles` and not in the needle, and six prose lines were counted
    # as sites with no role. Stated as arithmetic so a third population cannot
    # appear quietly.
    reader_total, assert_total, decl_total = rust_role_totals()
    if sum(rust_census().values()) != reader_total + assert_total + decl_total:
        failed += 1
        print(
            f"FAIL: the Rust census counts {sum(rust_census().values())} site(s) "
            f"and the role tally accounts for "
            f"{reader_total + assert_total + decl_total} — one population, two "
            "answers",
            file=sys.stderr,
        )

    retyped_n, single_n = rust_reader_duplication()
    if retyped_n + single_n != reader_total:
        failed += 1
        print(
            f"FAIL: the duplication split counts {retyped_n + single_n} Rust "
            f"reader(s) and the role tally counts {reader_total} — one "
            "population, two answers",
            file=sys.stderr,
        )
    # ★★★★★ R2179 — the split's DISCRIMINATION is proven by fixtures, not by the
    # live tree. The arm that stood here failed whenever either bucket was
    # empty, so it would have gone red the day the campaign legitimately left no
    # single reader: a gate that fails on success, which is R2174's fixture
    # lesson in a second place. Each case is asked in BOTH directions, because
    # "can these name the same address" is symmetric and a matcher that is not
    # has a bug.
    denote_cases: list[tuple[str, tuple[str, str], bool]] = [
        ("a concrete literal names itself",
         ("chart.series.0", "chart.series.0"), True),
        ("and not its neighbour",
         ("chart.series.0", "chart.series.1"), False),
        ("★ a template names its concrete member — the unit this repaired",
         ("records.row.{r}", "records.row.7"), True),
        ("and with two placeholders",
         ("lab.scenario.{lane}.{at}", "lab.scenario.main.2"), True),
        ("a placeholder's NAME does not matter",
         ("feed.row.{n}", "feed.row.{index}"), True),
        ("★★ a longer sibling in the same family is not the same address",
         ("feed.row.{n}", "feed.row.{n}.cell.{k}"), False),
        ("★★ a placeholder never crosses a dot",
         ("feed.row.{n}", "feed.row.3.cell.1"), False),
        ("a placeholder beside literal text, inside one segment",
         ("feed.head.col#{n}", "feed.head.col#0"), True),
        ("and that literal text still has to agree",
         ("feed.head.col#{n}", "feed.head.col_label#0"), False),
        ("★ two templates that overlap at DIFFERENT positions",
         ("a.{x}x.y", "a.z{y}.y"), True),
        ("a placeholder fills at least one character",
         ("lab.advanced.{name}", "lab.advanced."), False),
        ("a prefix names itself",
         ("lab.advanced.", "lab.advanced."), True),
    ]
    for label, (left, right), want in denote_cases:
        for a, b in ((left, right), (right, left)):
            if may_denote_same(a, b) != want:
                failed += 1
                print(
                    f"FAIL: {label}: may_denote_same({a!r}, {b!r}) -> "
                    f"{not want}, wanted {want}",
                    file=sys.stderr,
                )
                break
    # ★★★★★ R2181 — the UNANCHORED shape and the matcher that says which of those
    # sites are certainly paint. Fixtures, for the reason above; and the matcher
    # is held to `may_denote_same` wherever both apply, because they are two
    # implementations of one rule and only one of them had cases.
    # ★★★★★ R2184 — the TEMPLATE family: which literals are one, what stem they
    # take, and the two questions asked of a template stem.
    family_cases: list[tuple[str, str, str | None]] = [
        ("a runtime value in the family segment", "card.{id}.config", "card.{}"),
        ("★ an instance key on it is dropped, as a concrete family's is",
         "card.{kind}#{n}.close", "card.{}"),
        ("a placeholder beside text in the family segment keeps the text",
         "lab.form_{x}.row", "lab.form_{}"),
        ("a concrete family stays concrete", "card.alarms#6.feed", "card.alarms"),
        ("★★ a handed prefix is not one", "{tag}.row.{slot}", None),
        ("★★★★★ R2192 — a word and a placeholder that NO declaration composes is "
         "a prefix, as two words are (`state.{i}` is not: `state.<index>` is "
         "declared, and `declared_cases` holds that half)", "unheard_of.{i}", None),
        ("a sentence is not one", "the card.{id}.x", None),
    ]
    for label, literal, want in family_cases:
        got = family_of(literal)
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: family_of({literal!r}) -> {got!r}, wanted {want!r}",
                file=sys.stderr,
            )
    # ★★★★★ R2189 — a mounted path is judged behind its separator. The
    # separator is handed in so no case depends on today's tree, and then the
    # tree's own is read and used, so the rule and its SSOT are held together.
    mount_cases: list[tuple[str, str, str]] = [
        ("a root mount", "/external/node.0.x", "node.0.x"),
        ("a tagged mount", "/grid/external/value.3", "value.3"),
        ("a window prefix and an index chain",
         "/window[main]/0/1/external/selected.0", "selected.0"),
        ("an unmounted literal is itself", "node.0.x", "node.0.x"),
        ("★★ a sentence that mentions a mounted path stays prose",
         "see /external/node.0.x", "see /external/node.0.x"),
        ("an unresolved mount carries no separator", "{}/node.{}.x", "{}/node.{}.x"),
    ]
    for label, literal, want in mount_cases:
        got = address_part(literal, "/external/")
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: address_part({literal!r}) -> {got!r}, wanted {want!r}",
                file=sys.stderr,
            )
    sep = mount_separator()
    tree_mount_cases: list[tuple[str, object, object]] = [
        ("★ the tree's separator is read and mounts a path",
         address_part(f"/grid{sep}card.alarms.feed"), "card.alarms.feed"),
        ("★★ and a family is charged behind it",
         family_of(f"/panel{sep}expanded.cat.Physics"), "expanded.cat"),
        ("but a composed path behind it is not a spelling", family_of(f"{sep}{{}}"), None),
    ]
    for label, got, want in tree_mount_cases:
        if got != want:
            failed += 1
            print(f"FAIL: {label}: {got!r}, wanted {want!r}", file=sys.stderr)
    # ★★★★★ R2187 — a numeric-id path is charged by the DECLARATIONS it
    # composes, handed in here so no case depends on today's tree.
    declared = (
        ("value.<index>", "value.{}"),
        ("node.<id>.op", "node.{}.op"),
        ("node.<id>.resolved_input.<port>", "node.{}.resolved_input.{}"),
        ("checked.<path>", "checked.{}"),
    )
    declared_cases: list[tuple[str, str, str | None]] = [
        ("a two-segment path with a numeric argument", "value.3", "value.{}"),
        ("a numeric id with a field after it", "node.0.op", "node.{}"),
        ("★ an argument mid-path and one at the end",
         "node.2.resolved_input.1", "node.{}"),
        ("★★ a numeric id with a RUNTIME argument later in the path — the dry "
         "run counted these and the first implementation did not",
         "node.2.resolved_input.{}", "node.{}"),
        ("★★ but a runtime placeholder where the declaration has a WORD composes "
         "nothing: a placeholder in the literal is text to the matcher",
         "node.0.{}", None),
        ("★★ a field the screen does not declare composes nothing",
         "node.0.bogus", None),
        ("★★ a head nothing declares composes nothing — the shape is not enough",
         "elem.0", None),
        ("a version string is not the shape", "1.2.3", None),
        ("a word in the family segment is ADDRESS's question, not this one",
         "value.elem.1", None),
        ("a sentence is not the shape", "value 3", None),
        # ★★★★★ R2192 — a runtime argument where the id goes.
        ("★★ a two-segment path with a RUNTIME argument", "value.{}", "value.{}"),
        ("★ a Rust placeholder carries its name and is the same argument",
         "value.{idx}", "value.{}"),
        ("★★ a runtime argument on a head nothing declares composes nothing",
         "elem.{i}", None),
        ("text beside a placeholder is not an argument segment", "value.x{i}", None),
        # ★★★★★ R2195 — an argument runs to the declaration's next fixed piece,
        # as the runtime reads it.
        ("★★ a trailing argument may hold a dot: a menu path", "checked.2.0", "checked.{}"),
        ("★ and may be written with runtime values", "checked.{}.{}", "checked.{}"),
        ("★ a mid-path argument still stops at the fixed piece after it",
         "node.2.0.op", "node.{}"),
        ("★★ but an argument cannot swallow a fixed piece the declaration requires",
         "node.2.0.bogus", None),
    ]
    for label, literal, want in declared_cases:
        got = declared_path_family(literal, declared)
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: declared_path_family({literal!r}) -> {got!r}, "
                f"wanted {want!r}",
                file=sys.stderr,
            )
    # ★★★★★ R2195 — the runtime rule is a WIDENING of the one-segment rule, held
    # over the tree's own declarations: wherever an argument is one segment the
    # two answer alike, so switching changed only dotted arguments.
    one_segment = [
        declared
        for declared, template in declared_path_templates()
        if template_denotes(template, template.replace("{}", "7"))
        != declared_path_addresses(declared, template.replace("{}", "7"))
    ]
    rule_agreement_cases: list[tuple[str, object, object]] = [
        ("★★ the runtime rule and the one-segment rule agree on every shipped "
         "declaration wherever an argument is one segment",
         one_segment, []),
        ("and the tree has declarations for that to be asked of",
         bool(declared_path_templates()), True),
    ]
    for label, got, want in rule_agreement_cases:
        if got != want:
            failed += 1
            print(f"FAIL: {label}: {got!r}, wanted {want!r}", file=sys.stderr)
    template_stem_cases: list[tuple[str, object, object]] = [
        ("★ a template family is held by an address in any of its families",
         covers("card.alarms.feed", "card.{}"), True),
        ("and inside a longer address, as a concrete stem is",
         covers("x.card.alarms.feed", "card.{}"), True),
        ("but its word must be a whole segment",
         covers("cardx.alarms.feed", "card.{}"), False),
        ("text beside the placeholder must agree",
         covers("lab.form_x.row", "lab.form_{}"), True),
        ("and does not answer for other text",
         covers("lab.formx.row", "lab.form_{}"), False),
        ("★ a Rust format template spells its template family",
         rust_needles('fn f(id: &str) { t(format!("card.{id}.x")); }', "card.{}"),
         (True, True)),
        ("and a comment spells it in the source only",
         rust_needles("// see card.{id}.x\nfn f() {}", "card.{}"), (True, False)),
        ("a concrete stem keeps its needle",
         rust_needles('fn f() { t("card.alarms.x"); }', "card.alarms"), (True, True)),
    ]
    for label, got, want in template_stem_cases:
        if got != want:
            failed += 1
            print(f"FAIL: {label}: got {got!r}, wanted {want!r}", file=sys.stderr)
    shape_cases: list[tuple[str, str, bool]] = [
        ("★★★★★ R2184 — a literal-headed template is a family now, not unanchored",
         "card.{id}.config", False),
        ("a handed prefix at the head", "{tag}.row.{slot}", True),
        ("★ a handed prefix and one member is an address too", "{tag}.head", True),
        ("an anchored address is not unanchored", "lab.form.remove.{key}", False),
        ("★ nor is one with a templated instance key", "card.alarms#{n}.feed", False),
        ("a word and a placeholder is a prefix, as two words are", "state.{i}", False),
        ("a sentence is not the shape", "the seat at {x}.y", False),
        ("a path is not the shape", "scene/{x}.y", False),
        ("a lone placeholder is not", "{}", False),
        ("a head that is not ONE placeholder is not", "{a}{b}.row.x", False),
        ("an escaped brace is text", "{{tag}}.row.x", False),
        ("nothing but placeholders names nothing", "{}.{}", False),
    ]
    for label, literal, want in shape_cases:
        if unanchored_shape(literal) != want:
            failed += 1
            print(
                f"FAIL: {label}: unanchored_shape({literal!r}) -> {not want}",
                file=sys.stderr,
            )
    compose_cases: list[tuple[str, tuple[str, str], bool]] = [
        ("★ a handed prefix fills WHOLE segments",
         ("{tag}.row.{slot}", "card.alarms#0.feed.row.0"), True),
        ("but at least one of them",
         ("{tag}.row.{slot}", "row.0"), False),
        ("a family placeholder fills one segment",
         ("card.{id}.feed", "card.alarms#0.feed"), True),
        ("and never crosses a dot",
         ("card.{id}.feed", "card.alarms#0.x.feed"), False),
        ("a template is not its longer sibling",
         ("card.{id}.feed", "card.alarms#0.feed.row.0"), False),
    ]
    for label, (template, address), want in compose_cases:
        if template_denotes(template, address) != want:
            failed += 1
            print(
                f"FAIL: {label}: template_denotes({template!r}, {address!r}) -> "
                f"{not want}",
                file=sys.stderr,
            )
    for label, (left, right), want in denote_cases:
        for template, concrete in ((left, right), (right, left)):
            if _PLACEHOLDER.search(concrete) or _PLACEHOLDER.fullmatch(
                template.split(".")[0]
            ):
                continue
            if template_denotes(template, concrete) != want:
                failed += 1
                print(
                    f"FAIL: {label}: template_denotes and may_denote_same disagree "
                    f"on ({template!r}, {concrete!r})",
                    file=sys.stderr,
                )
    # A pin reader that found nothing would call every unanchored site unpinned,
    # and a fold form nobody taught would leave a `*` no placeholder can match —
    # both silent. Asserted against the tree, which carries both fold forms.
    held_addresses = pinned_addresses()
    if not held_addresses or any("*" in address for address in held_addresses):
        failed += 1
        print(
            f"FAIL: pinned_addresses read {len(held_addresses)} address(es), "
            "or left a fold marker in one — teach it the fold form",
            file=sys.stderr,
        )
    distinct = len({literal for _where, _stem, literal in address_literal_sites()})
    if distinct < 100:
        failed += 1
        print(
            f"FAIL: only {distinct} distinct address literal(s) were read — this "
            "is not the corpus, and every reader would look like the only site "
            "naming its address",
            file=sys.stderr,
        )

    vocab_cases: list[tuple[str, str, str]] = [
        ("an emitted grammar's family is paint", "chart.candle", "paint"),
        ("a screen pin's family is paint", "lab.frame", "paint"),
        ("a parametric declaration's family is a path", "value.elem", "path"),
        # ★★★★★ R2174 — a stem NO round can declare, and that is the repair.
        # This case named `nodegroups.node` until the round that declared it,
        # and it would have named whichever family came next: a fixture whose
        # subject is "nothing declares this" cannot be a live family in a tree
        # whose whole campaign is to declare them all. The three cases above
        # anchor on live declarations because being live is what they assert;
        # this one asserts the ABSENCE of one, so its subject has to be absent
        # by construction. It still fails if the classifier ever starts
        # guessing a vocabulary rather than answering `unknown`.
        ("★ and one nothing declares is neither, said as such", "absent.family", "unknown"),
    ]
    for label, stem, want in vocab_cases:
        if site_vocabulary(stem) != want:
            failed += 1
            print(
                f"FAIL: {label}: site_vocabulary({stem!r}) -> "
                f"{site_vocabulary(stem)!r}, wanted {want!r}",
                file=sys.stderr,
            )

    live_pins = pin_sources(set(read_budget()) | set(census()))
    for source in ("artifact", "schema", "assertion", ""):
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

    # ★★★★★ R2164 — THE TWO HALVES MUST AGREE ABOUT THE SAME LINE, and they
    # did not. `rust_roles` learned at R2158 that a file the parent declares
    # behind `#[cfg(test)]` is assertions throughout; `asserted_families` kept
    # its own copy of the rule, requiring a literal `#[cfg(test)]` INSIDE the
    # file. So the same line was an assertion when the census tallied roles and
    # nothing at all when it asked what pinned the family — and the second
    # answer is the one that writes BLOCKED, this file's word for work no round
    # may finish. Every case below is DERIVED from the tree; none names a file.
    test_only = test_only_modules()
    if not test_only:
        failed += 1
        print(
            "FAIL: no test-only module was found — every case below is vacuous",
            file=sys.stderr,
        )
    # (1) a test-only module has no READER in it. That is what the words mean,
    #     and it is the invariant the pin question depends on.
    leaked = []
    for path in sorted(test_only):
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if rust_roles(path, body)[0]:
            leaked.append(str(path.relative_to(ROOT)))
    if leaked:
        failed += 1
        print(
            f"FAIL: {leaked} are declared test-only and still count readers",
            file=sys.stderr,
        )
    # (2) the blind spot itself: a test-only module need not contain the string
    #     `#[cfg(test)]` at all, and the old rule skipped such a file whole.
    #     Every stem such a file spells must be asserted. If NO such file
    #     exists this case proves nothing, so its absence is a failure.
    asserted = asserted_families()
    invisible = 0
    for path in sorted(test_only):
        try:
            body = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        sites = rust_sites_in(body)
        if not sites or "#[cfg(test)]" in body:
            continue
        invisible += 1
        for _line, stem in sites:
            if not asserted.get(stem):
                failed += 1
                print(
                    f"FAIL: {path.relative_to(ROOT)} asserts {stem!r} and the "
                    "pin question cannot see it",
                    file=sys.stderr,
                )
    if not invisible:
        failed += 1
        print(
            "FAIL: no test-only module lacks an in-file `#[cfg(test)]`, so the "
            "case that found R2164's defect has no failing path any more",
            file=sys.stderr,
        )
    # (3) and the totals reconcile: what the role tally calls an assertion over
    #     the wide corpus is exactly what the pin question counts. This is the
    #     contradiction stated as arithmetic, so it cannot come back as a third
    #     copy of the rule somewhere else.
    wide_assertions = 0
    for root in RUST_ROOTS:
        for path in sorted((ROOT / root).rglob("*.rs")):
            try:
                body = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            wide_assertions += rust_roles(path, body)[1]
    if wide_assertions != sum(asserted.values()):
        failed += 1
        print(
            f"FAIL: the role tally counts {wide_assertions} assertion(s) over "
            f"the wide corpus and the pin question counts "
            f"{sum(asserted.values())} — one line, two answers",
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
        # ★★★★★ R2178 — a key handed to the OWNER CACHE names a state slot, not
        # a mark. The first five are the shapes the rule must recognise, two of
        # them copied from the tree's own `.cache` calls; the last five are the
        # ones it must NOT swallow, and they are what keep a name rule from
        # becoming a blanket exemption.
        (
            "★ a key handed to the owner cache is not a site",
            'owner.cache("todomvc.persistence.boot", move || State::load());',
            [],
        ),
        (
            "★ nor through a turbofish with a nested generic, across lines",
            'let s = owner.cache::<Rc<State>, _>(\n    "a.b.c",\n    State::new,\n);',
            [],
        ),
        (
            "★ measured shape: two levels of generics",
            'let w = owner.cache::<Signal<Vec<WindowSpec>>,_>("a.b.c", f);',
            [],
        ),
        (
            "★ measured shape: a path-qualified type in the turbofish",
            'o.cache_get_by_str::<pinion_core::widgets::caret_blink::CaretBlink>("a.b.c")',
            [],
        ),
        (
            "★ and `cache_contains` is the same namespace",
            'o.cache_contains::<u32>("a.b.c")',
            [],
        ),
        (
            "★★ a real tag on the same line as a cache key is still a site",
            'owner.cache("hover_anim", f); node.with_tag("lab.reset.view");',
            ["lab.reset"],
        ),
        (
            "★★ a free function called `cache` is not the owner's method",
            'cache("lab.reset.view")',
            ["lab.reset"],
        ),
        (
            "★★ nor is a UFCS call, which errs towards counting",
            'Owner::cache(&owner, "lab.reset.view")',
            ["lab.reset"],
        ),
        (
            "★★ a literal that is not the FIRST argument is still a site",
            'owner.cache(key, || tag("lab.reset.view"))',
            ["lab.reset"],
        ),
        (
            "★★ a generic method outside the set is still a site",
            'node.with_tag::<Rc<S>>("lab.reset.view")',
            ["lab.reset"],
        ),
        (
            "★★★★★ R2181 — an instance key belongs to the family segment",
            'fn f() { press("card.alarms#6.feed"); }',
            ["card.alarms"],
        ),
        (
            "★★★★★ R2181 — a placeholder naming a same-file &str const is its value",
            'const CHART_TAG: &str = "chart";\n'
            'fn f(k: usize) -> String { format!("{CHART_TAG}.label.x.{k}") }',
            ["chart.label"],
        ),
        (
            "★★ a `pub(crate) static` binds one too, inside a module",
            "mod voice {\n    pub(crate) static TAG: &'static str = \"audio\";\n}\n"
            'fn f() { n(format!("{TAG}.voice.{id}")); }',
            ["audio.voice"],
        ),
        (
            "★★ a name bound to two values in one file is a scoping question, "
            "and is left alone",
            'mod a {\n    const TAG: &str = "one";\n}\nmod b {\n    const TAG: &str = "two";\n}\n'
            'fn f() { t(format!("{TAG}.x.y")); }',
            [],
        ),
        (
            "★★ a const holding a brace is text at runtime, never a placeholder",
            'const T: &str = "a.{}";\nfn f() { t(format!("{T}.x.y")); }',
            [],
        ),
        (
            "★ a format spec is not a capture this substitutes",
            'const TAG: &str = "lab";\nfn f() { t(format!("{TAG:>4}.x.y")); }',
            [],
        ),
        (
            "a lowercase local is a runtime value, not a constant",
            'fn f(tag: &str) { t(format!("{tag}.row.{slot}")); }',
            [],
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
        # ★ R2157 — the RUST arm, which answers zero on this tree for the same
        # reason and so proves nothing by running either.
        rust_caught = [key for _line, key in rust_config_keys_in(
            f'    row("{dotted[0]}", "bool"),\n'
        )]
        if rust_caught != [dotted[0]]:
            failed += 1
            print(
                f"FAIL: a Rust reader spelling {dotted[0]!r} was not caught: "
                f"{rust_caught}",
                file=sys.stderr,
            )
        rust_comment = rust_config_keys_in(f'    // about "{dotted[0]}"\n')
        if rust_comment:
            failed += 1
            print(
                f"FAIL: a Rust comment naming {dotted[0]!r} was counted: "
                f"{rust_comment}",
                file=sys.stderr,
            )

    # ★★★★★ R2160 — **BLOCKED is a claim about pins, and it must not drift into
    # a claim about anything else.** Both directions are asserted: a family this
    # calls blocked must really have readers and really have no pin, and a
    # family with a pin must never be called blocked. Without the second arm a
    # bug that returned everything would read as "the campaign is impossible"
    # and be believed, because nobody re-derives a number that large.
    blocked = blocked_families()
    pins_now = pin_sources([stem for stem, _w, _r in blocked])
    for stem, walk_sites, rust_sites in blocked:
        if pins_now[stem]:
            failed += 1
            print(
                f"FAIL: {stem} is reported BLOCKED and is pinned by "
                f"{pins_now[stem]!r}",
                file=sys.stderr,
            )
            break
        if not (walk_sites or rust_sites):
            failed += 1
            print(
                f"FAIL: {stem} is reported BLOCKED and has no reader at all — "
                "a converted family is not blocked, it is done",
                file=sys.stderr,
            )
            break
    # ⚠ The other arm, over the whole population rather than the answer: every
    # family with readers and no pin must BE in the list.
    #
    # 🟥🟥🟥 ★★★★★ R2175 — **this arm RE-SPELLED the rule, with the same wrong
    # population as the code, so it could not fail.** It said "every family with
    # readers" and then computed that from `census()` / `rust_census()`, which
    # count assertions and declarations too — the identical mistake the function
    # it was guarding had made. Two checks, one wrong population: this project's
    # own standing lesson (*a gate must not re-spell the rule; it must compare
    # against the DERIVATION's output*), reproduced inside the gate written to
    # enforce it. It now asks `reducible_sites` — the one derivation — so the
    # gate and the code cannot be wrong in the same direction without the
    # arithmetic arm below noticing.
    left_now = reducible_sites()
    should = {
        stem
        for stem, source in pin_sources(sorted(left_now)).items()
        if not source
    }
    missed = sorted(should - {stem for stem, _w, _r in blocked})
    if missed:
        failed += 1
        print(
            f"FAIL: {missed} have readers and no pin and are not reported "
            "BLOCKED — a round would pick them as ordinary work",
            file=sys.stderr,
        )
    # ★★★★★ R2175 — and the ARITHMETIC arm, which is what makes the two above
    # more than a restatement: the per-family derivation must sum to the same
    # queue the grand-total role tally gives. One population, one answer — the
    # form this file already uses for the census/role tally, applied to the pair
    # that actually diverged.
    reader_total = rust_role_totals()[0]
    walk_total_now = sum(census().values())
    if sum(sum(pair) for pair in left_now.values()) != walk_total_now + reader_total:
        failed += 1
        print(
            f"FAIL: reducible_sites sums to "
            f"{sum(sum(pair) for pair in left_now.values())} and the role tally "
            f"says the queue is {walk_total_now + reader_total} — one "
            "population, two answers",
            file=sys.stderr,
        )
    # ★ And BLOCKED must be a SUBSET of the queue, which is the property whose
    # absence let 19 declarations be reported as work no round may finish.
    #
    # ⚠ Stated rather than hidden: this arm and the one after it share a
    # derivation with what they check, so today they cannot fail — see
    # `debt-a-check-whose-two-sides-share-one-derivation`. They are kept because
    # the failure they pin is a RE-IMPLEMENTATION (`blocked_families` reading
    # `rust_scan` again, which is exactly what R2175 repaired), and against that
    # change they do fire. The arm above is the one that compares two
    # independent walks over the corpus, and it is the load-bearing one.
    outside = sorted(
        stem for stem, _w, _r in blocked if stem not in left_now
    )
    if outside:
        failed += 1
        print(
            f"FAIL: {outside} are reported BLOCKED and are not in the reducible "
            "queue at all — BLOCKED qualifies the queue, so it cannot name "
            "something outside it",
            file=sys.stderr,
        )
    # ★★★★★ R2175 — the work order's `pin` word, as a PURE rule with fixtures.
    # The discriminating pair is the last two: no pin and work is BLOCKED, no
    # pin and no work is not.
    word_cases = [
        ("a pinned family answers with its source", ("artifact", True), "artifact"),
        ("and does so whether or not it owes", ("assertion", False), "assertion"),
        ("★ nothing pinning it AND work left is BLOCKED", (None, True), "BLOCKED"),
        ("★★ nothing pinning it and NO work left is not", (None, False), "unpinned"),
    ]
    for label, (source, has_work), want in word_cases:
        got = pin_word(source, has_work)
        if got != want:
            failed += 1
            print(
                f"FAIL: {label}: pin_word({source!r}, {has_work!r}) -> {got!r}, "
                f"wanted {want!r}",
                file=sys.stderr,
            )
    # ★ The wider question keeps its own name and must stay WIDER — if these two
    # ever answer the same set, one of them has stopped asking its question.
    wider = {stem for stem, _w, _r in unpinned_families()}
    if not {stem for stem, _w, _r in blocked} <= wider:
        failed += 1
        print(
            "FAIL: a BLOCKED family is not reported as unpinned — the narrow "
            "question must be a subset of the wide one",
            file=sys.stderr,
        )

    # ★★★★★ R2158 — **the test-only module rule, which moves sites in the
    # direction that can HIDE work.** R2144's warning is on the record: a
    # classifier that calls a production reader an assertion understates the
    # debt. This one is exact rather than heuristic, and these assertions are
    # what says so — every file it claims must really be declared
    # `#[cfg(test)] mod <name>;` by a sibling, and a file nobody declares that
    # way must not be in the set.
    modules = test_only_modules()
    if not modules:
        failed += 1
        print(
            "FAIL: no module file is declared behind `#[cfg(test)]` anywhere in "
            "this tree, so the rule that reclassifies them is doing nothing",
            file=sys.stderr,
        )
    declared_names: set[str] = set()
    for parent in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/src/**/*.rs")):
        body = parent.read_text(encoding="utf-8", errors="replace")
        for name in re.findall(r"#\[cfg\(test\)\]\s*\n\s*mod\s+([a-z_0-9]+)\s*;", body):
            declared_names.add(str(parent.parent / f"{name}.rs"))
    claimed = {str(path) for path in modules if str(path).endswith(".rs")}
    overclaimed = sorted(
        path
        for path in claimed
        if path.startswith(str(ROOT / RUST_CENSUS_ROOT))
        and Path(path).is_file()
        and path not in declared_names
    )
    if overclaimed:
        failed += 1
        print(
            f"FAIL: {overclaimed} are treated as test-only and no sibling "
            "declares them behind `#[cfg(test)]` — the rule is claiming files "
            "it was not told about, which UNDERSTATES the reader debt",
            file=sys.stderr,
        )
    # ⚠ And the other arm: a production module must not be swept up. `main.rs`
    # is nobody's `#[cfg(test)] mod`, and if it ever lands here the rule has
    # stopped reading declarations.
    for stray in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/src/main.rs")):
        if stray in modules:
            failed += 1
            print(
                f"FAIL: {stray.relative_to(ROOT)} is treated as test-only",
                file=sys.stderr,
            )
            break

    # ★★★★★ R2157 — **the population's BOUNDARY, asserted rather than trusted.**
    # `pinion-core` spells four of these paths 57 times as fixtures for a generic
    # config-form widget; it has no option surface and is right to invent a key.
    # A later round that widens this needle to the whole tree would turn those 57
    # into findings whose only "fix" is making the framework depend on a demo. So
    # what is asserted is the rule that keeps them out: a package is in the
    # population iff it can ASK.
    consumers = config_key_consumers()
    consuming = {
        path.relative_to(ROOT / RUST_CENSUS_ROOT).parts[0] for path in consumers
    }
    artifact = Path(CONFIG_SURFACE_ARTIFACTS).name
    if not consuming:
        failed += 1
        print(
            "FAIL: no package compiles the option surface in, so the config-key "
            "population is empty and its gate checks nothing",
            file=sys.stderr,
        )
    for package in sorted((ROOT / RUST_CENSUS_ROOT).glob("*/")):
        reaches = any(
            artifact in path.read_text(encoding="utf-8", errors="replace")
            for path in package.glob("src/**/*.rs")
        )
        if reaches != (package.name in consuming):
            failed += 1
            print(
                f"FAIL: {package.name} {'reaches' if reaches else 'cannot reach'}"
                f" the option surface and is "
                f"{'absent from' if reaches else 'inside'} the config-key "
                "population — the boundary is supposed to BE that question",
                file=sys.stderr,
            )
    if any(path.name in RUST_DECLARING_FILES for path in consumers):
        failed += 1
        print(
            "FAIL: a declaring file is inside the config-key population, so the "
            "declaration is charged for declaring",
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
    # ★★★★★ R2176 — and the excused file's OWN gate must still be switched on.
    # See `public_declaring_modules`: `pub mod` in a binary example makes
    # `dead_code` stop seeing the composers inside, which is what enforces
    # R2158's ruling that a composer with no consumer is not a declaration.
    # Measured at R2176: one of the two binary examples had diverged, and the
    # flip named a dead function immediately.
    exported = public_declaring_modules()
    if exported:
        failed += 1
        print(
            f"FAIL: a binary example declares its declaring module `pub`, where "
            f"`dead_code` cannot see inside it: {exported} — write `mod "
            "<name>;`, so the compiler refuses a composer nothing calls",
            file=sys.stderr,
        )

    # ★★★★★ R2178 — the ratchet's THIRD state: a stem spelled only as a slot
    # name was never an address family, so it has no budget row and it is never
    # a deleted check. Without this, correcting the needle would have filed two
    # owner-cache keys into `docs/unpinned-families.tsv` and grown the
    # unreachable half of the closing criterion from two false positives to
    # four.
    #
    # ⚠ Stated rather than hidden: the `filed` half shares its derivation with
    # `deleted_checks` and cannot fail today (see
    # `debt-a-check-whose-two-sides-share-one-derivation`). The `carried` half
    # reads the COMMITTED budget and fails whenever a re-pin was skipped. What
    # both pin is a re-implementation of `write_budget` or `deleted_checks`
    # that forgets the third state.
    other = non_address_stems()
    budget_rows = set(read_budget())
    carried = sorted(other & budget_rows)
    filed = sorted(
        other
        & set(
            deleted_checks(
                census(), rust_census(), budget_rows | set(census()) | set(rust_census())
            )
        )
    )
    if carried or filed:
        failed += 1
        print(
            f"FAIL: stem(s) spelled only as a non-address name are treated as "
            f"address families — carried in the budget: {carried}; filed as "
            f"deleted checks: {filed}. They were never families, so neither "
            "may name them. Run --write-budget.",
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

    # ⚠ R2144 warned that this sum is HAND-MAINTAINED and that a case list added
    # without a term here is a printed number covering less than it claims. It
    # then happened again: `grammar_cases` landed at R2153 and was never a term,
    # so for ten rounds this line said 53 while 53 + those cases ran — and R2163
    # added six more to it without the number moving. ⇒ ★ a warning beside a
    # hand-maintained number is not a gate; the population has to be DERIVED.
    #
    # The two trailing constants are ad-hoc assertions with no list to count,
    # and they are NAMED rather than folded into one number nobody can read.
    #
    # ★★★★★ R2189 — the case lists are DERIVED from what this function defines,
    # every list named `cases` or `*_cases`. The tuple this replaced was a
    # hand-kept list and it had drifted: `shape_cases`, `compose_cases`,
    # `family_cases`, `template_stem_cases` and `declared_cases` (R2181-R2187)
    # ran and could fail while the total never counted them, so the "N of N"
    # this prints undercounted, and a counterfactual that failed five of R2189's
    # mount cases printed "133 of 138". A new case list is counted by being
    # named like one.
    scope = dict(locals())
    case_lists = [
        value
        for name, value in scope.items()
        if (name == "cases" or name.endswith("_cases")) and isinstance(value, list)
    ]
    total = sum(len(case_list) for case_list in case_lists) + (
        5  # R2147/R2166: four classifier words and the artifact corpus floor
        + 1  # R2166: the parametric-declaration corpus floor
        + 1  # R2167: no family is claimed by BOTH vocabularies
        + 1  # R2170: the census total and the role tally are one population
        + 2  # R2168: the split's arithmetic and corpus-size arms (R2179 moved
        #      its discrimination arm to the `denote_cases` fixtures)
        + 4  # R2164: the role rule's four derived cross-checks
        + 3  # R2175: the queue's arithmetic arm and BLOCKED's two subset arms
        + 1  # R2176: a binary example's declaring module must stay private
        + 1  # R2178: a slot-name stem has no budget row and is no deleted check
        + 10  # the ad-hoc assertions above, pre-existing and left alone
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
