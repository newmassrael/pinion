#!/usr/bin/env python3
"""R2129 — **one read path, one wire shape**, across the screens of one product.

    python3 tools/read_path_shapes.py             # the ratchet (pre-push)
    python3 tools/read_path_shapes.py --selftest  # this tool's own tests
    python3 tools/read_path_shapes.py --list      # every shared name and its shapes
    python3 tools/read_path_shapes.py --write-budget   # after a deliberate change

# What this counts

An `ExternalIntrospect::query` arm answers one of `IntrospectValue`'s
variants. Nothing makes two screens that publish the SAME path name answer the
same variant, and for 400 rounds two of them did not: five screens and the
shell answered `spec` as a `Json` while `hello-node-lab` answered a `Text`
holding JSON. The R2128 measurement is what that costs — one walk asked two
screens for `spec` in one file, section A got a dict and section B a string,
and the same expression that was right eight lines up raised `TypeError`.

Nobody chose that. `git log -S` says the `Text` arrived at R1724 with no
mention of the shape in its commit body, R1729 gave the sibling screen a
`Json`, and the four screens after it followed a decision that was never
written down. ⇒ this is the gate that would have said so at the fifth.

# FOUR checks over FOUR populations, and only one of them has a budget

★★★★★ The populations are separate because the claims are. The first draft
gave all of them the product's seven crates, and that is the defect R2128 had
just paid for one level up — one list answering two questions, right until the
questions part. Here it was worse than wrong: it left the gate unable to catch
the very walk this round was written about.

1. **SPLIT — across the assembled product.** A name two screens publish in two
   different shapes. Product-scoped because that IS the claim: a client walks
   from one screen to the next, and two unrelated demos both publishing `reset`
   owe each other nothing. PINNED in `docs/read-path-shapes.tsv`; a new split
   or one that grew a shape is refused, a repaired one is dropped with
   `--write-budget`, so the list only falls.
2. **BROKEN PROMISE — across every `ExternalIntrospect` impl in the workspace.**
   A `SchemaField::new("name", "type")` its own arm contradicts. ZERO, with no
   budget: a split needs two screens to agree on which is wrong, and a promise
   is one impl contradicting itself, so there is nothing to negotiate. R1637
   made the declaration a precondition of dispatch, which is what makes a wrong
   type there worse than an arm's — a client is told the type before it reads.

   ★ The first run found two, both in `hello-node-lab`: `spec` declared
   `string` where its five siblings declare `json`, and `review` declared
   `json` while its arm answered a `Text`. The second had **no reader at all**,
   so nothing was ever going to trip over it.

   ⚠⚠ **And the wider population immediately found two defects in THIS TOOL,
   not in the tree.** (a) `IntrospectValue::Raw` was classified as a seventh
   word; `Raw` IS json — `kind()` in `pinion-core` says
   `Json(_) | Raw(_) => "json"` — so two correct surfaces read as broken.
   (b) Pairing was per FILE, and `crates/pinion-core/src/external.rs` holds
   SIX impls, so one fixture's `count` declaration was compared with another
   fixture's arm. Every analyzer screen is one impl per file, which is exactly
   why the coarse rule looked right for as long as the population was small.
3. **STALE DECODE — across every walk.** `json.loads` of a read the screens
   that walk drives answer as JSON, which is a `TypeError` waiting for the next
   sweep. ZERO, no budget. The sweep does not gate the push (R835); this is the
   half that does.
4. **OFF-VOCABULARY TYPE WORD — across every source that spells a schema
   constructor.** R2131. A declared word no `SchemaType` variant spells. ZERO,
   no budget, and the legal spellings are read off `SchemaType::as_str` rather
   than listed here, so the gate cannot drift from the framework.

   ★★★★★ **The widest population of the four, and it earns the width twice.**
   It is not scoped to `ExternalIntrospect` impls, because "is this word in the
   vocabulary?" has meaning wherever the word is written — a const table in a
   module with no impl, a test fixture, a `SchemaArg` in a helper — whereas
   "does this surface contradict itself?" needs a surface. And it is not scoped
   to READS: `SchemaArg` and the invoke channel spell the same vocabulary.

   ⚠⚠ **It exists because the compiler's own refusal arrives too late.** Since
   R2131 `SchemaType::parse` panics in a `const fn`, and the expectation
   carried into that round was that an unknown word is therefore a compile
   error at the declaration line. Measured: true for `const ITEM: SchemaField =
   ..`, and NOT true for `IntrospectSchema::new(const { &[..] })`, which is the
   shape essentially every surface here uses — an inline `const {}` block is
   evaluated at codegen, and `cargo check` and `cargo clippy` do not codegen.
   ⇒ the compiler closes the vocabulary at BUILD time; this closes it at the
   gate. See [`type_tokens_declared`] for the table.

# The derivation

**The population is derived, not listed.** It is `hello-analyzer-shell` plus
the `hello-*` path dependencies it declares, transitively — the crates that
are assembled into one product, which is the only scope in which "one name,
one shape" is a claim at all. Two unrelated demos may both publish `reset`
and owe each other nothing.

For each crate, every `fn query(&self, path: &str)` body's `match path` block
is scanned brace-aware (arms are multi-line and hold braces of their own), and
each arm's body is classified by the `IntrospectValue` constructors it can
reach. A name published by two crates under two different classifications is a
SPLIT.

# ⚠ What this census cannot see, stated rather than discovered later

* **An arm that delegates** — `"spec" | "rail" => read_specification(path)` —
  reaches no constructor here, so it is `unknown` and excluded from the
  comparison. The count of those is printed, because a blind spot that is
  silent cannot be told from one that is empty.
* **An arm that can answer two variants** by its own branching is `mixed`, and
  `mixed` is compared as itself: two screens that both branch agree here even
  if they branch differently. What this checks is the shape a caller must be
  ready for, not the value.
* Classification is by TEXT within the arm. A helper that returns
  `IntrospectValue` is invisible, which is the `unknown` case above.
* `invoke` arms are NOT scanned. An action's return shape is a different
  question and the same words appear in both matches.

# The ratchet

It PINS the backlog rather than demanding zero: `docs/read-path-shapes.tsv`
carries one row per split that exists, and the gate refuses a NEW split or a
split that grew a shape. A split repaired must be dropped from the budget with
`--write-budget`, so the pin can only fall.

★★★★★ **There is a SECOND pin, and it is on what this census never looked at**
(R2130): `docs/read-path-unseen.tsv`. Everything above answers questions about
the population that WAS examined, and none of them notices a declaration
LEAVING it. Add one new spelling of a type word and that word's declarations
move from `compared` into `skipped` — `broken` stays 0, every check here stays
green, and coverage has fallen with nothing said. That is not hypothetical:
R2129 measured two real `object`-declared / `Text`-answering mismatches that
the existing vocabulary gap was already hiding. So `0 broken` is reported
together with the size of what it is silent about, and a RISE in that size is
refused exactly as a new split is.

⛔ The repair for a rise is never to teach this file another synonym. That
blesses the split vocabulary and re-hides what it hides; the root is that
`SchemaField.ty` is a `&'static str` so any word is legal, and closing that
vocabulary drives these rows to zero by leaving nothing to skip.

★ The runtime end of the same invariant is `rpc_verify.screen_spec`, which
refuses a string rather than converting it. Two ends, one rule, and each says
so — the door cannot silently absorb what this would refuse.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = ROOT / "examples"
BUDGET = ROOT / "docs" / "read-path-shapes.tsv"

#: R2130 — the population this census CANNOT examine, pinned so it can only
#: fall. `BUDGET` pins what the census found; this pins what it never looked
#: at, which is the number a reader needs in order to know what `0 broken` is
#: a statement ABOUT. Printing it (R2129.2) was half the job: a printed number
#: nothing bounds can rise while every verdict stays green, and it rises in
#: the direction that HIDES work — a declaration that moves from `compared`
#: into `skipped` takes any disagreement it was carrying out of sight with it.
UNSEEN = ROOT / "docs" / "read-path-unseen.tsv"

#: The root of the assembled product. Its path dependencies are the population;
#: this one name is the only thing about the population written down here.
PRODUCT = "hello-analyzer-shell"

#: An arm's text is searched for these, longest spelling first so that
#: `IntrospectValue::Int` cannot be read out of `IntrospectValue::Integer`-like
#: neighbours by prefix. The right-hand side is what a caller must be ready for.
#: The file that owns which variants are the same shape.
CORE_EXTERNAL = ROOT / "crates" / "pinion-core" / "src" / "external.rs"

#: ★★★★★ R2131 — THE SECOND HAND TABLE IS GONE TOO, and this one was joined to
#: the wrong half of the framework.
#:
#: It read: `IntrospectValue::kind()` answers a fragment a person reads in a
#: sentence ("a whole number"), a `SchemaField` declares a token an agent
#: matches on ("int"), and this table was the join. `kind()`'s own docstring
#: says the two renderings are held to *opposite* rules -- so a census deriving
#: a machine vocabulary was reading the PERSON-facing words and re-spelling the
#: machine ones. It worked, and it was one reworded sentence away from not
#: working; `selftest` held it total precisely because nothing else could.
#:
#: R2131 gave the framework a machine-side answer, `SchemaType`, so
#: [`variant_groups`] now reads `SchemaType::of` directly and the join has no
#: middle. The exclusion of `null` that this table made by omission is now made
#: by name, which is the difference between a rule and a gap.

#: Local one-word constructors the SCREENS wrap a variant in. Unlike the table
#: above these are not the framework's — each is a closure or helper a screen
#: declares for itself (`let text = |s: String| Ok(IntrospectValue::Text(s));`)
#: — so there is nothing upstream to derive them from, and a text census cannot
#: follow a call. That is the `unknown` blind spot the module docstring states.
_HELPERS: tuple[tuple[str, str], ...] = (
    ("text", "text"),
    ("int", "int"),
    ("plain", "text"),
)

#: ★★★★★ R2131 — THIS TABLE IS GONE. It listed the word a `SchemaField`
#: declares against the arm shape that keeps its promise, by hand, and the
#: comment justifying the hand-writing read as principle: *inventing the
#: mapping for one would be this file deciding the framework's vocabulary.*
#: True, and the conclusion was backwards — the framework did not HAVE a
#: vocabulary to defer to, because `SchemaField.ty` was `&'static str`. So this
#: file was deciding it anyway, in silence, five words at a time, and every word
#: outside the five was skipped rather than compared.
#:
#: R2131 closed the vocabulary as `pinion_core::external::SchemaType`, and
#: [`declared_as`] now READS it — `SchemaType::as_str` for the token, plus
#: `SchemaType::of` and `IntrospectValue::kind` for the arm it admits. Deriving
#: is what makes "the framework's vocabulary" a thing this tool can defer to
#: instead of a thing it asserts.

_ARM = re.compile(
    r"^[ \t]*((?:\"[A-Za-z_0-9]+\"[ \t]*\|[ \t]*)*\"[A-Za-z_0-9]+\")[ \t]*=>[ \t]*",
    re.M,
)


# ── pure rules ───────────────────────────────────────────────────────────────


def path_deps(manifest: str) -> list[str]:
    """The `hello-*` crates this manifest depends on by path, in file order."""
    return [
        m.group(1)
        for m in re.finditer(
            r"^(hello-[A-Za-z0-9_-]+)\s*=\s*\{[^}\n]*path\s*=", manifest, re.M
        )
    ]


@lru_cache(maxsize=64)
def code_mask(source: str) -> tuple[bool, ...]:
    """`True` at every index that is CODE — outside strings, chars and comments.

    ★ Load-bearing, not tidiness. Every scan below counts brackets, and this
    tree's arms hold `text(format!("{x},{y}"))` and comments that spell a brace
    in prose. A counter that read those would close an arm in the middle of one
    and hand the next arm's text to the classifier — which is how the first
    draft reported `hello-analyzer-shell` answering `spec` as `text`: its arm
    delegates, and the scan had swallowed the `text(...)` arm after it.
    """
    mask = [True] * len(source)
    i, n = 0, len(source)
    while i < n:
        two = source[i : i + 2]
        if two == "//":
            end = source.find("\n", i)
            end = n if end < 0 else end
            mask[i:end] = [False] * (end - i)
            i = end
        elif two == "/*":
            end = source.find("*/", i + 2)
            end = n if end < 0 else end + 2
            mask[i:end] = [False] * (end - i)
            i = end
        elif source[i] == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == "\\" else 1
            mask[i : j + 1] = [False] * (j + 1 - i)
            i = j + 1
        elif source[i] == "'" and i + 2 < n:
            # A char literal, or a lifetime. `'a'` and `'\n'` are literals and
            # can hold a bracket; `'a` in `&'a str` is not and must stay code.
            if source[i + 2] == "'":
                mask[i : i + 3] = [False] * 3
                i += 3
            elif source[i + 1] == "\\" and i + 3 < n and source[i + 3] == "'":
                mask[i : i + 4] = [False] * 4
                i += 4
            else:
                i += 1
        else:
            i += 1
    # A TUPLE, and cached: every scan below asks for the same file's mask
    # several times over (the consts, each impl, each arm), and rebuilding a
    # 700 KB file's mask each time was most of this tool's runtime. Immutable
    # because a cached mutable would hand every caller the same list.
    return tuple(mask)


def query_blocks(source: str) -> list[str]:
    """Every `match path { … }` body belonging to a `query(&self, path: &str)`.

    Brace-aware rather than line-based: an arm holds braces of its own and the
    largest of these matches is over 700 lines, so a regex that stopped at the
    first `}` would read a tenth of one screen and call the rest absent.
    """
    mask = code_mask(source)
    out: list[str] = []
    for head in re.finditer(r"fn query\(\s*&self,\s*path: &str", source):
        start = source.find("match path {", head.end())
        if start < 0:
            continue
        i = start + len("match path {")
        depth = 1
        while i < len(source) and depth:
            if mask[i]:
                depth += {"{": 1, "}": -1}.get(source[i], 0)
            i += 1
        out.append(source[start + len("match path {") : i - 1])
    return out


def arms(block: str) -> list[tuple[tuple[str, ...], str]]:
    """Each `"name" | "name" => body` in a match block, body kept whole.

    An arm's body is either a BLOCK — which ends at its matching brace and
    needs no comma after it — or an expression, which runs to the comma that
    closes it at depth zero. Reading the second rule for the first is what made
    a delegating arm absorb the arm after it.
    """
    mask = code_mask(block)
    out: list[tuple[tuple[str, ...], str]] = []
    i = 0
    while i < len(block):
        head = _ARM.search(block, i)
        if not head:
            break
        names = tuple(re.findall(r"\"([A-Za-z_0-9]+)\"", head.group(1)))
        j = head.end()
        while j < len(block) and block[j] in " \t\n\r":
            j += 1
        if j < len(block) and block[j] == "{" and mask[j]:
            depth = 0
            while j < len(block):
                if mask[j]:
                    depth += {"{": 1, "}": -1}.get(block[j], 0)
                    if depth == 0 and block[j] == "}":
                        j += 1
                        break
                j += 1
        else:
            depth = 0
            while j < len(block):
                # ⚠ `block[j] if mask[j] else ""` reads as the same thing and is
                # not: `"" in "{(["` is TRUE, so every masked character counted
                # as an opener and one arm swallowed the other forty.
                if mask[j]:
                    ch = block[j]
                    if ch in "{([":
                        depth += 1
                    elif ch in "})]":
                        if depth == 0:
                            break
                        depth -= 1
                    elif ch == "," and depth == 0:
                        break
                j += 1
        out.append((names, block[head.end() : j].strip()))
        i = j + 1
    return out


#: The `SchemaType` variants that are not an ARM SHAPE, and why each is not.
#:
#: Spelled out rather than left to fall through a lookup, because that is the
#: difference between a rule and a gap: the table this replaced excluded `null`
#: by simply not listing it, and an omission cannot say whether it was a
#: decision or an oversight.
_NOT_A_READ_SHAPE: dict[str, str] = {
    # An answer carrying no value. A path that can answer `Null` still declares
    # the type it answers otherwise, so `Null` grades nothing -- and
    # `SchemaType::admits` says the same thing from the framework's side.
    "Null": "an absent value, not a shape",
    # The placeholder a `const fn` composer overwrites; never declared.
    "Unset": "a slot nobody has written yet",
}


def variant_groups(core_source: str) -> dict[str, str]:
    """`IntrospectValue` variant -> the shape token it satisfies, DERIVED.

    ★★★★★ Derived rather than written down, because writing it down is how this
    tool's own first defect happened: it classified `Raw` as a seventh word when
    the framework already groups `Json(_) | Raw(_)`, and two CORRECT surfaces
    read as broken promises. A census that invents a vocabulary the framework
    does not have reports its own invention.

    ⚠⚠ R2131 MOVED THE SOURCE, and the old one was the wrong half of the
    framework. This used to read `IntrospectValue::kind()` and join its answers
    through a hand table — but `kind()` answers *person* words ("a whole
    number") and its own docstring insists they are held to opposite rules from
    the schema's tokens. A machine vocabulary was being recovered from prose, so
    a reworded sentence could have moved it.

    `SchemaType::of` is the machine-side answer and did not exist until R2131.
    It is the right source for the same reason `kind()` was: it carries no
    wildcard on purpose, so the round that adds an `IntrospectValue` variant
    meets a compile error there — and therefore this sees every variant the enum
    has, not every variant somebody remembered.

    The shape token is the `SchemaType` variant lowercased, which is an identity
    rather than a coincidence: the census's shapes and the framework's types are
    the same partition of the same enum. Note `Text` -> `text` while the token
    it DECLARES is `string` — the two differ, on purpose, and [`declared_as`] is
    where they are joined.
    """
    return {
        value_variant: schema_variant.lower()
        for value_variant, schema_variant in schema_type_of(core_source).items()
        if schema_variant not in _NOT_A_READ_SHAPE
    }


def _match_block(source: str, signature: str, subject: str) -> str:
    """The body of the `match <subject> {` that opens a fn matching `signature`."""
    mask = code_mask(source)
    head = re.search(signature, source)
    if not head:
        return ""
    opener = f"match {subject} {{"
    start = source.find(opener, head.end())
    if start < 0:
        return ""
    i, depth = start + len(opener), 1
    while i < len(source) and depth:
        if mask[i]:
            depth += {"{": 1, "}": -1}.get(source[i], 0)
        i += 1
    return source[start + len(opener) : i - 1]


def schema_type_tokens(core_source: str) -> dict[str, str]:
    """`SchemaType` variant -> the wire token it publishes, read off `as_str`."""
    out: dict[str, str] = {}
    for line in _match_block(
        core_source, r"fn as_str\(\s*self\s*\)", "self"
    ).splitlines():
        arm = re.match(r'\s*Self::(\w+)\s*=>\s*"([^"]*)"', line)
        if arm:
            out[arm.group(1)] = arm.group(2)
    return out


def schema_type_of(core_source: str) -> dict[str, str]:
    """`IntrospectValue` variant -> the `SchemaType` variant, read off `of`."""
    out: dict[str, str] = {}
    for line in _match_block(
        core_source, r"fn of\(value: &IntrospectValue\)", "value"
    ).splitlines():
        arm = re.match(
            r"\s*((?:IntrospectValue::\w+(?:\(_?\))?\s*\|\s*)*"
            r"IntrospectValue::\w+(?:\(_?\))?)\s*=>\s*Self::(\w+)",
            line,
        )
        if not arm:
            continue
        for variant in re.findall(r"IntrospectValue::(\w+)", arm.group(1)):
            out[variant] = arm.group(2)
    return out


def declared_as(core_source: str) -> dict[str, str]:
    """The declared TOKEN -> the arm shape that keeps its promise, DERIVED.

    ★★★★★ R2131 — the hand table this replaces is described where it used to
    sit. The chain is three links, each read off the framework rather than
    restated here:

    | link | source |
    |---|---|
    | `IntrospectValue` variant -> shape | `kind()`, via [`variant_groups`] |
    | `IntrospectValue` variant -> `SchemaType` | `SchemaType::of` |
    | `SchemaType` -> wire token | `SchemaType::as_str` |

    Composing them answers *what must an arm answer for this declared word to
    be kept*, and it answers it for every word the vocabulary has — which is
    the property the hand table could not have. A word missing here is now a
    word the ENUM does not carry, and since R2131 the enum is what the compiler
    holds callers to, so such a word cannot be declared at all.

    `null` drops out on its own and correctly: `kind()` answers `"null"`, which
    is not a shape any read arm is classified as, so a path declaring `null`
    has nothing to be compared against. That is the same exclusion the hand
    table made deliberately, arrived at rather than remembered.
    """
    groups = variant_groups(core_source)
    tokens = schema_type_tokens(core_source)
    of = schema_type_of(core_source)
    out: dict[str, str] = {}
    for value_variant, shape in groups.items():
        schema_variant = of.get(value_variant)
        token = tokens.get(schema_variant) if schema_variant else None
        if token:
            out[token] = shape
    return out


def shape_of(body: str, groups: dict[str, str]) -> str:
    """What a caller must be ready for: one variant, `mixed`, or `unknown`.

    `unknown` is an arm that hands the answer to a helper — the census cannot
    follow a call, and saying so is better than guessing a variant from a
    function's name.

    ★ Comments are not read. An arm of this tree routinely carries five lines
    of prose naming the variant a NEIGHBOUR answers, so a classifier that read
    them would classify the neighbour.
    """
    mask = code_mask(body)
    code = "".join(c if m else " " for c, m in zip(body, mask))
    # The variant spellings come from `groups`, which `variant_groups` read off
    # `kind()`. A `Raw` is `json` here because the framework says so, not
    # because this file decided. The lowercase constructor fn (`raw(..)`) is
    # matched too -- it is the same variant under a builder's name.
    found = {
        shape
        for variant, shape in groups.items()
        if f"IntrospectValue::{variant}" in code
        or f"IntrospectValue::{variant.lower()}" in code
    }
    found |= {
        shape
        for name, shape in _HELPERS
        if re.search(rf"(?<![A-Za-z0-9_]){re.escape(name)}\(", code)
    }
    if not found:
        return "unknown"
    if len(found) > 1:
        return "mixed"
    return found.pop()


def _balanced(
    source: str, mask: tuple[bool, ...], start: int, open_ch: str, close_ch: str
) -> int:
    """Index just past the bracket opened at `start`."""
    depth, i = 0, start
    while i < len(source):
        if mask[i]:
            if source[i] == open_ch:
                depth += 1
            elif source[i] == close_ch:
                depth -= 1
                if depth == 0:
                    return i + 1
        i += 1
    return len(source)


def const_items(source: str) -> dict[str, str]:
    """Every `const NAME: … = …;` item's value text, keyed by NAME.

    Needed because a schema is usually not written inside `schema()`: this
    tree's screens declare `const FIELDS: &[SchemaField] = &{ … };` at module
    level and the impl says `IntrospectSchema::new(FIELDS)`. A census that read
    only the function body would find no declaration for the largest screen in
    the tree and report it as having nothing to check.
    """
    mask = code_mask(source)
    out: dict[str, str] = {}
    for m in re.finditer(r"\bconst\s+([A-Z][A-Za-z0-9_]*)\s*:", source):
        if not mask[m.start()]:
            continue
        eq = source.find("=", m.end())
        if eq < 0:
            continue
        depth, i = 0, eq
        while i < len(source):
            if mask[i]:
                ch = source[i]
                if ch in "{([":
                    depth += 1
                elif ch in "})]":
                    depth -= 1
                elif ch == ";" and depth == 0:
                    break
            i += 1
        out[m.group(1)] = source[eq + 1 : i]
    return out


def impl_bodies(source: str) -> list[str]:
    """The body of every `impl ExternalIntrospect for …` block.

    ★★★★★ The unit a declaration is paired with. Pairing per FILE was this
    tool's second defect and it only showed on a wider population:
    `crates/pinion-core/src/external.rs` holds SIX of these, so a `count`
    declared `string` by one fixture was compared against `Int` answered by
    another and reported as a broken promise. Every analyzer screen is one
    impl per file, which is exactly why the coarser rule looked right.
    """
    mask = code_mask(source)
    out: list[str] = []
    for head in re.finditer(r"\bimpl(?:<[^>]*>)?\s+ExternalIntrospect\s+for\b", source):
        if not mask[head.start()]:
            continue
        brace = source.find("{", head.end())
        if brace < 0:
            continue
        out.append(source[brace : _balanced(source, mask, brace, "{", "}")])
    return out


def schema_text(impl_body: str, consts: dict[str, str]) -> str:
    """The text an impl DECLARES from — its `schema()` plus the consts it names.

    A const is pulled in when the impl mentions it by name, which is how
    `IntrospectSchema::new(FIELDS)` reaches its own field list. Pulling in only
    the named ones is what keeps a second fixture's const out.
    """
    mask = code_mask(impl_body)
    head = re.search(r"fn schema\(\s*&self", impl_body)
    if not head:
        return ""
    brace = impl_body.find("{", head.end())
    if brace < 0:
        return ""
    body = impl_body[brace : _balanced(impl_body, mask, brace, "{", "}")]
    named = [v for k, v in consts.items() if re.search(rf"\b{re.escape(k)}\b", body)]
    return body + "\n".join(named)


def declared_types(source: str) -> dict[str, str]:
    """What each `SchemaField::new("name", "type")` in this file PROMISES.

    The declaration is the other half of a read path. R1637-R1640 made it a
    precondition of dispatch — a path a screen has not declared cannot be
    called — so a declaration that names the wrong type is the self-describing
    surface telling a client something false, which is worse than an arm
    disagreeing with another screen: there is no second screen to compare it
    with.

    ⚠⚠ R2131 — THE TYPE CLASS IS `[A-Za-z_0-9]*`, AND THE OLD `[a-z]+` WAS A
    HOLE BELOW R2130's PIN. A token with a digit in it did not fail to match the
    type group; it failed to match the whole pattern, so the declaration was not
    *skipped* — it was INVISIBLE, counted neither among the examined nor among
    the pinned unexamined, and `0 broken over 843 examined and 45 pinned` was a
    statement about a population with a silent hole in it.

    Measured: `crates/pinion-shell/tests/dispatch_core.rs` declared
    `SchemaField::new("value", "i32")`, a fifth off-vocabulary word. R2130 saw
    `i32` in a regex census, judged it an artifact of `re.S` matching across
    lines, and dismissed it — the dismissal was wrong, and nothing here could
    have contradicted it, because this extractor could not see the site either.
    ⇒ ★ a population defined by a pattern is only as wide as the pattern, so the
    pattern is part of the claim.

    The empty string is allowed (`*`, not `+`): `SchemaField::EMPTY` and the
    composers that spell `SchemaField::new("", "")` declare `SchemaType::Unset`,
    and an extractor that could not see those would have the same hole again.
    """
    return {
        m.group(1): m.group(2)
        for m in re.finditer(
            r"SchemaField::new\(\s*\"([A-Za-z_0-9.<>]*)\"\s*,\s*\"([A-Za-z_0-9]*)\"",
            source,
        )
    }


def broken_promises(
    declared: dict[str, str],
    answered: dict[str, str],
    vocabulary: dict[str, str],
) -> dict[str, tuple[str, str]]:
    """`name -> (declared word, answered shape)` where the two disagree.

    A shape the census could not classify is not a disagreement, for the reason
    it is not a split: `unknown` is this census's limit, not the screen's
    defect.

    A declared word absent from `vocabulary` is still left alone here and
    counted by [`unknown_tokens`] instead — but since R2131 that is a much
    stronger statement than it was. `vocabulary` is now DERIVED from
    `SchemaType` rather than hand-listed, and `SchemaType` is what the compiler
    holds every declaration to, so a word outside it can no longer be declared
    at all. The branch is kept because the derivation can legitimately exclude a
    word (`null` has no read shape), not because the vocabulary might grow
    behind this tool's back.
    """
    out: dict[str, tuple[str, str]] = {}
    for name, word in declared.items():
        shape = answered.get(name)
        want = vocabulary.get(word)
        if shape in (None, "unknown", "mixed") or want is None:
            continue
        if want != shape:
            out[name] = (word, shape)
    return out


def unknown_tokens(
    declared: dict[str, str],
    answered: dict[str, str],
    vocabulary: dict[str, str],
) -> dict[str, int]:
    """The declared words this file has no shape for, counted by word.

    🟥 THE SILENCE THIS REPLACES WAS A HOLE, NOT A BLIND SPOT. `broken_promises`
    skips a token it does not recognise, and the comment justifying that read as
    principled -- *the vocabulary is the framework's and may grow*. Measured at
    R2129.2 it was hiding **29 declarations in four crates**, because the schema
    vocabulary has SYNONYMS: `text_field.rs` declares `boolean`/`number`/`object`
    where the tree declares `bool`/`int`/`json`, and `pinion-narrative` declares
    `text` where the tree declares `string`. One of the pairs it hid was a real
    contradiction -- two paths declared `object` whose arms answer a `Text`.

    ⚠ The repair was NOT to add the synonyms to the table. That would compare
    all of them and make the number look right while blessing a split
    vocabulary and putting it back out of sight -- which is precisely the door
    `screen_spec` used to be, and what let this class live 400 rounds. A census
    counts what it cannot read; it does not absorb it.

    ✅ R2131 TOOK THE OTHER REPAIR AND THIS NOW COUNTS ZERO. `SchemaField.ty` is
    a closed `SchemaType`, the five synonyms (`boolean` 46, `text` 20,
    `number` 17, `object` 13, `i32` 1) were canonicalised, and `vocabulary` is
    derived from the enum by [`declared_as`]. Two of those declarations were
    real broken promises the gap had been hiding -- `said` in
    `hello-key-patterns` and `hello-log-view` declared `object` over an arm
    answering `Text` -- and both are repaired rather than reclassified.

    ⚠ IT IS KEPT, NOT DELETED, and the reason is the one this file keeps
    learning: a count that can only be zero is still the thing that says so.
    `declared_as` excludes a token with no read shape (`null`), and a future
    `SchemaType` variant reaches declarations before it reaches `kind()`. This
    is what would report that, and R2130's pin is what refuses it.
    """
    out: dict[str, int] = {}
    for name, word in declared.items():
        if name in answered and word not in vocabulary:
            out[word] = out.get(word, 0) + 1
    return out


#: Every constructor that takes a type token, and WHICH argument it is.
#:
#: `SchemaArg`'s are here for the reason its own doc gives — the argument's type
#: is "in the same vocabulary as `SchemaField::ty`" — and since R2131 that is
#: the same `SchemaType`, so a census of one that skipped the other would be
#: measuring half a vocabulary.
_TYPED_CONSTRUCTORS: dict[str, int] = {
    "SchemaField::new": 1,
    "SchemaField::action": 1,
    "SchemaField::parametric": 1,
    "SchemaField::action_with": 1,
    "SchemaField::send": 0,
    "SchemaArg::key": 1,
    "SchemaArg::open": 1,
    "SchemaArg::one_of": 1,
    "SchemaArg::one_of_with": 1,
}

_STRING_LITERAL = re.compile(r'^"([^"\\]*)"$')


def type_tokens_declared(source: str) -> dict[str, int]:
    """Every type token this source spells, counted — arguments SPLIT, not grepped.

    ★★★★★ R2131 — THE ONE GATE THAT DOES NOT WAIT FOR CODEGEN, and it exists
    because of a measurement that contradicted the round's own plan.

    Closing the vocabulary made `SchemaType::parse` panic in a `const fn`, and
    the claim carried forward from R2130 was that an unknown word therefore
    becomes "a compile error AT THE DECLARATION LINE". Measured on the real
    tree, that is true of only one of the two shapes a schema is written in:

    | shape | `cargo check` (`--emit=metadata`) | a build |
    |---|:--:|:--:|
    | `const ITEM: SchemaField = SchemaField::new(..)` | refuses | refuses |
    | `IntrospectSchema::new(const { &[SchemaField::new(..)] })` | **passes** | refuses |

    An inline `const {}` block in a fn body is evaluated at codegen, which
    `cargo check` and `cargo clippy` do not do — and the second shape is what
    essentially every surface in this tree uses. R2130's experiment used the
    first, which is why the generalisation looked safe. ⇒ the compiler closes
    the vocabulary at BUILD time, and between an edit and a build there was
    nothing. This is that nothing.

    It also reaches two populations the compiler's own refusal does not: a crate
    no profile builds, and `SchemaArg`, whose tokens sit in the same vocabulary.

    Arguments are split by depth on a comment- and string-masked source rather
    than matched by a regex, because the tokens are at a fixed ARGUMENT INDEX
    and not at a fixed distance: `SchemaField::action_with(path, ty, form,
    args)` puts a slice after the token, and `re.S` across a call is how R2130
    came to report a word that was not there while missing one that was.
    """
    mask = code_mask(source)
    out: dict[str, int] = {}
    for name, index in _TYPED_CONSTRUCTORS.items():
        for head in re.finditer(re.escape(name) + r"\s*\(", source):
            if not mask[head.start()]:
                continue
            open_at = source.index("(", head.end() - 1)
            close_at = _balanced(source, mask, open_at, "(", ")")
            if close_at < 0:
                continue
            depth, cuts, at = 0, [open_at + 1], open_at
            while at < close_at:
                if mask[at]:
                    if source[at] in "([{":
                        depth += 1
                    elif source[at] in ")]}":
                        depth -= 1
                    elif source[at] == "," and depth == 1:
                        cuts.append(at)
                at += 1
            args = [
                source[a + 1 if n else a : b].strip()
                for n, (a, b) in enumerate(zip(cuts, cuts[1:] + [close_at - 1]))
            ]
            if len(args) <= index:
                continue
            literal = _STRING_LITERAL.match(args[index])
            # A non-literal is a `const` spelled elsewhere; it reaches this
            # census through that definition's own site, so counting the
            # forwarding call would double-count it.
            if literal:
                out[literal.group(1)] = out.get(literal.group(1), 0) + 1
    return out


def off_vocabulary(counted: dict[str, int], spellings: set[str]) -> dict[str, int]:
    """The declared tokens no `SchemaType` variant spells."""
    return {word: n for word, n in counted.items() if word not in spellings}


def decoding_sites(source: str) -> tuple[list[tuple[int, str]], int]:
    """`([(line, path)], unresolved)` for every `json.loads` of a WIRE READ.

    ⚠⚠ A **2-tuple, deliberately**: you cannot ask this for the sites without
    being handed the size of what it could not resolve. The first draft
    returned the list alone and its docstring claimed the caller counted the
    blind spot — no caller did, and the claim sat one line above the code that
    disproved it. That is this round's own subject (a number written beside the
    thing that answers it and never asked), so the shape is what fixes it
    rather than a sentence asking the next author to remember.

    ★★★★★ Structural, with `ast`, and the round that wrote it learnt why the
    hard way. R2129's first population was `grep 'json.loads.*spec'`, which
    found twenty-two walks and missed a twenty-third: `r1732` wraps the decode
    in a helper, so the path literal and the `json.loads` are on different
    lines and no line holds both. The sweep found it as a `TypeError` after the
    repair was already written.

    Two levels, because that is what the tree has:

    * **direct** — `json.loads(tf.query(f"{EXT}/spec"))`, the literal read out
      of the same expression;
    * **wrapped** — a local helper whose body decodes a wire read, and then the
      literal its CALLERS pass.

    A site whose path is not a literal cannot be resolved — a parametric
    decoder called with a variable, or a decode at module level with nothing
    to supply one. Those are the second element, and `--check` prints it on
    every green run: measured at R2129 it is ZERO across 726 walks, and
    zero-by-measurement is a different fact from zero-by-luck.
    """
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return []

    def reads_wire(node: ast.AST, wrappers: set[str]) -> bool:
        for sub in ast.walk(node):
            if not isinstance(sub, ast.Call):
                continue
            func = sub.func
            if isinstance(func, ast.Attribute) and func.attr == "query":
                return True
            if isinstance(func, ast.Name) and func.id in wrappers:
                return True
        return False

    def path_literal(node: ast.AST) -> str | None:
        """The last path-shaped string constant in an expression."""
        found = None
        for sub in ast.walk(node):
            if isinstance(sub, ast.Constant) and isinstance(sub.value, str):
                word = sub.value.rsplit("/", 1)[-1]
                if word and re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", word):
                    found = word
            elif isinstance(sub, ast.JoinedStr):
                for part in sub.values:
                    if isinstance(part, ast.Constant) and isinstance(part.value, str):
                        word = part.value.rsplit("/", 1)[-1]
                        if word and re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", word):
                            found = word
        return found

    fns = [f for f in ast.walk(tree) if isinstance(f, ast.FunctionDef)]
    readers = {f.name for f in fns if reads_wire(f, set())}
    decoders: set[str] = set()
    sites: list[tuple[int, str]] = []
    unresolved = 0
    for call in ast.walk(tree):
        if not isinstance(call, ast.Call) or not call.args:
            continue
        func = call.func
        if not (isinstance(func, ast.Attribute) and func.attr == "loads"):
            continue
        if not reads_wire(call.args[0], readers):
            continue
        literal = path_literal(call.args[0])
        if literal:
            sites.append((call.lineno, literal))
            continue
        # No literal here, so the path is the CALLER's. The enclosing function
        # is a parametric decoder; if it has none, nothing can supply one.
        enclosing = [
            fn.name
            for fn in fns
            if fn.lineno <= call.lineno <= (fn.end_lineno or call.lineno)
        ]
        if enclosing:
            decoders.update(enclosing)
        else:
            unresolved += 1
    for call in ast.walk(tree):
        if isinstance(call, ast.Call) and isinstance(call.func, ast.Name):
            if call.func.id not in decoders:
                continue
            literals = [
                a.value
                for a in call.args
                if isinstance(a, ast.Constant) and isinstance(a.value, str)
            ]
            if literals:
                sites += [(call.lineno, lit) for lit in literals]
            else:
                unresolved += 1
    return sorted(set(sites)), unresolved


def decode_is_stale(
    published: dict[str, dict[str, str]], driven: set[str], path: str
) -> bool:
    """Every screen THIS WALK drives answers `path` as a JSON value.

    ★★★★★ Joined through the screens the walk actually launches, not through
    every screen in the product. The first draft asked the second question and
    could not catch the very site the sweep had just found: `spec` is answered
    `json` by six surfaces and `unknown` by the shell (its arm delegates), so
    "every screen answers json" was false and the walk that drives ONLY the
    node lab went unreported. The walk's own targets are `demo_radius`'s
    derivation, reused rather than spelled a second time here.

    `unknown` is dropped rather than counted: it is this census's limit, and a
    limit is not evidence that a screen answers text. A walk driving nothing
    this census classified is left alone — which is a blind spot, and the
    caller prints how many walks it read so the silence has a size.
    """
    by_crate = published.get(path, {})
    known = {s for c, s in by_crate.items() if c in driven and s != "unknown"}
    return known == {"json"}


def splits(published: dict[str, dict[str, str]]) -> dict[str, dict[str, str]]:
    """The names two crates answer in two DIFFERENT knowable shapes.

    `unknown` is not a shape and never makes a split by itself: an arm the
    census cannot classify is a blind spot, and reporting it as a disagreement
    would be reporting the census's own limit as the product's defect.
    """
    out: dict[str, dict[str, str]] = {}
    for name, by_crate in published.items():
        known = {c: s for c, s in by_crate.items() if s != "unknown"}
        if len(known) > 1 and len(set(known.values())) > 1:
            out[name] = known
    return out


def budget_verdict(
    now: dict[str, dict[str, str]], pinned: dict[str, dict[str, str]]
) -> tuple[list[str], list[str], list[str]]:
    """`(new, grown, repaired)` — what rose, what widened, what may be dropped."""
    new = sorted(n for n in now if n not in pinned)
    grown = sorted(
        n
        for n, by in now.items()
        if n in pinned and set(by.items()) - set(pinned[n].items())
    )
    repaired = sorted(n for n in pinned if n not in now)
    return new, grown, repaired


def render_budget(now: dict[str, dict[str, str]]) -> str:
    """The budget file's text, so the writer and the selftest agree on it."""
    lines = [
        "# R2129 — read paths one product publishes in MORE THAN ONE wire shape.",
        "# One row per screen that publishes a split name, with the shape it",
        "# answers. Derived by `tools/read_path_shapes.py --write-budget`; do",
        "# not hand-edit. The gate refuses a NEW split and a split that grew a",
        "# shape, so this list can only fall.",
        "#",
        "# path\tcrate\tshape",
    ]
    for name, by_crate in sorted(now.items()):
        lines += [f"{name}\t{crate}\t{shape}" for crate, shape in sorted(by_crate.items())]
    return "\n".join(lines) + "\n"


def unseen_now(untyped: dict[str, int], blind: int) -> dict[str, int]:
    """The unexamined population as one table: what was skipped, and why.

    Two kinds, deliberately in ONE pin rather than two files. They have
    different causes — an unknown type word is a vocabulary that never
    closed ([[debt-a-declared-type-is-an-untyped-string]]), an unclassified
    arm is a delegation this census cannot follow — but they are the same
    FACT to a reader of `0 broken`: a declaration the comparison never
    reached. Splitting them into two budgets would let a round move weight
    from one to the other without either moving.
    """
    now = {f"type-word:{word}": n for word, n in untyped.items() if n}
    if blind:
        now["unclassified-arm"] = blind
    return now


def unseen_verdict(
    now: dict[str, int], pinned: dict[str, int]
) -> tuple[list[str], list[str], list[str]]:
    """`(new, grown, repaired)` — what appeared, what rose, what may be dropped.

    Deliberately the same shape as `budget_verdict`, because it is the same
    rule: a pin that can only fall. `grown` compares COUNTS rather than the
    set of shapes, which is the one difference the two populations force.
    """
    new = sorted(k for k in now if k not in pinned)
    grown = sorted(k for k, n in now.items() if k in pinned and n > pinned[k])
    repaired = sorted(k for k in pinned if now.get(k, 0) < pinned[k])
    return new, grown, repaired


def render_unseen(now: dict[str, int]) -> str:
    """The unseen pin's text, so the writer and the selftest agree on it."""
    lines = [
        "# R2130 — the population `read_path_shapes.py` CANNOT examine.",
        "# `0 broken` is a statement about what is left after these, so the",
        "# count is pinned and the gate refuses a NEW kind or a RISEN count.",
        "# Derived by `tools/read_path_shapes.py --write-budget`; do not",
        "# hand-edit. Repairing one means re-running that, so this can only",
        "# fall.",
        "#",
        "# ⛔ Do NOT shrink these by teaching the census a synonym. That",
        "# blesses a split vocabulary and re-hides what the gap was hiding —",
        "# R2129 measured two real mismatches inside it. Close the vocabulary",
        "# instead; then these rows go to zero because there is nothing left",
        "# to skip.",
        "#",
        "# ✅ R2131 DID THAT, and the type-word rows are gone. `SchemaField.ty`",
        "# is a closed `SchemaType`, the five synonyms were canonicalised, and",
        "# the two mismatches the gap hid are repaired. The examined population",
        "# rose 843 -> 873: 29 that had been skipped, plus one the extractor's",
        "# own `[a-z]+` type class had made INVISIBLE rather than skipped.",
        "#",
        "# ⚠ What is left is the OTHER cause, and it is not a vocabulary",
        "# problem: an arm that hands its answer to a helper cannot be followed",
        "# by a text census at all ⇒ that half belongs to",
        "# debt-nothing-ties-a-declared-type-to-the-answered-variant.",
        "#",
        "# kind\tcount",
    ]
    lines += [f"{kind}\t{n}" for kind, n in sorted(now.items())]
    return "\n".join(lines) + "\n"


def parse_unseen(text: str) -> dict[str, int]:
    """The unseen pin read back, ignoring comments and blank lines."""
    out: dict[str, int] = {}
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        kind, count = line.split("\t")
        out[kind] = int(count)
    return out


def parse_budget(text: str) -> dict[str, dict[str, str]]:
    """The budget file read back, ignoring comments and blank lines."""
    out: dict[str, dict[str, str]] = {}
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        name, crate, shape = line.split("\t")
        out.setdefault(name, {})[crate] = shape
    return out


# ── oracles (impure: these are what decide WHAT is looked at) ─────────────────


def read_manifest(crate: str) -> str:
    """One example crate's `Cargo.toml`, or `""` when it has none."""
    path = EXAMPLES / crate / "Cargo.toml"
    return path.read_text(encoding="utf-8") if path.exists() else ""


def read_sources(crate: str) -> list[str]:
    """Every `.rs` file under one example crate, in a stable order."""
    return [
        p.read_text(encoding="utf-8")
        for p in sorted((EXAMPLES / crate / "src").rglob("*.rs"))
    ]


def read_budget() -> dict[str, dict[str, str]]:
    """The pinned splits, or `{}` on a tree that has no budget yet."""
    return parse_budget(BUDGET.read_text(encoding="utf-8")) if BUDGET.exists() else {}


def read_unseen() -> dict[str, int]:
    """The pinned unexamined population, or `{}` on a tree with no pin yet."""
    return parse_unseen(UNSEEN.read_text(encoding="utf-8")) if UNSEEN.exists() else {}


def read_core_source() -> str:
    """`pinion-core`'s `external.rs`, which owns `IntrospectValue::kind()`."""
    return CORE_EXTERNAL.read_text(encoding="utf-8")


def read_walks() -> list[Path]:
    """Every demo walk, in a stable order."""
    return sorted((ROOT / "tools" / "demos").glob("*.py"))


def declaration_sources() -> list[Path]:
    """Every workspace source that spells a schema constructor.

    ⚠ A WIDER POPULATION THAN [`promise_sources`], deliberately — that one asks
    "does this surface contradict itself?", which only has meaning where there
    is an `ExternalIntrospect` impl to contradict. This asks "is this word in
    the vocabulary?", which has meaning wherever the word is written: a const
    table in a module with no impl, a test fixture, a `SchemaArg` in a helper.
    Two questions, two populations. Sharing one would answer the narrower
    question under the wider one's name, which is the defect R2128 paid for and
    R2129 paid for again.
    """
    out: list[Path] = []
    for root in (ROOT / "crates", ROOT / "examples"):
        for path in sorted(root.rglob("*.rs")):
            if "Schema" in path.read_text(encoding="utf-8"):
                out.append(path)
    return out


def vocabulary_census() -> tuple[dict[str, dict[str, int]], dict[str, int], set[str]]:
    """`(off-vocabulary sites by file, every token counted, the legal spellings)`."""
    spellings = set(schema_type_tokens(read_core_source()).values())
    offenders: dict[str, dict[str, int]] = {}
    counted: dict[str, int] = {}
    for source_path in declaration_sources():
        here = type_tokens_declared(source_path.read_text(encoding="utf-8"))
        for word, n in here.items():
            counted[word] = counted.get(word, 0) + n
        bad = off_vocabulary(here, spellings)
        if bad:
            offenders[str(source_path.relative_to(ROOT))] = bad
    return offenders, counted, spellings


def stale_decodes(
    published: dict[str, dict[str, str]],
) -> tuple[list[tuple[str, int, str]], int]:
    """`([(walk, line, path)], unresolved)` — the stale decodes, and the blind spot.

    Both, for the reason [`decoding_sites`] returns both: a finding of zero is
    only worth as much as the number of sites the derivation could not read.
    """
    from demo_radius import launched_by

    out: list[tuple[str, int, str]] = []
    blind = 0
    for walk in read_walks():
        sites, unresolved = decoding_sites(walk.read_text(encoding="utf-8"))
        blind += unresolved
        if not sites:
            continue
        driven = launched_by(walk)
        out += [
            (walk.name, line, path)
            for line, path in sites
            if decode_is_stale(published, driven, path)
        ]
    return out, blind


def promise_sources() -> list[Path]:
    """Every workspace source that carries an `ExternalIntrospect` impl.

    Derived by reading, not by a list of crates: a surface is wherever someone
    wrote one, and this tree has them in `crates/` and in `examples/` alike.
    """
    out: list[Path] = []
    for root in (ROOT / "crates", ROOT / "examples"):
        for path in sorted(root.rglob("*.rs")):
            if "ExternalIntrospect" in path.read_text(encoding="utf-8"):
                out.append(path)
    return out


def product_crates() -> list[str]:
    """`PRODUCT` and every `hello-*` crate it reaches by path, transitively."""
    seen, queue = [], [PRODUCT]
    while queue:
        crate = queue.pop(0)
        if crate in seen:
            continue
        seen.append(crate)
        queue += [c for c in path_deps(read_manifest(crate)) if c not in seen]
    return seen


def answered_here(impl_body: str, groups: dict[str, str]) -> dict[str, str]:
    """The shape each read path is answered with, inside ONE impl."""
    out: dict[str, str] = {}
    for block in query_blocks(impl_body):
        for names, body in arms(block):
            shape = shape_of(body, groups)
            for name in names:
                out[name] = shape
    return out


def split_census() -> tuple[dict[str, dict[str, str]], list[str]]:
    """`(name -> crate -> shape, crates)` for the ASSEMBLED PRODUCT.

    Product-scoped, and that is the claim's scope rather than a convenience:
    two screens agree about a name because a client walks from one to the
    other. Two unrelated demos owe each other nothing.
    """
    crates = product_crates()
    groups = variant_groups(read_core_source())
    published: dict[str, dict[str, str]] = {}
    for crate in crates:
        for source in read_sources(crate):
            if "ExternalIntrospect" not in source:
                continue
            for impl_body in impl_bodies(source):
                for name, shape in answered_here(impl_body, groups).items():
                    was = published.get(name, {}).get(crate)
                    published.setdefault(name, {})[crate] = (
                        shape if was in (None, shape, "unknown") else "mixed"
                    )
    return published, crates


def promise_census() -> tuple[dict[str, tuple[str, str]], int, int, dict[str, int]]:
    """`(where -> broken promise, impls read, declarations compared)`.

    ★★★★★ THE WHOLE WORKSPACE, not the product — the two checks in this file
    were sharing a population and only one of them had a reason for it. A
    declaration contradicting its own arm is wrong wherever it sits; there is
    no second surface whose agreement makes it right. Scoping this to the
    product was R2128's defect happening again in the tool that round paid
    for: one list answering two questions, correct until the questions part.

    The unit is the IMPL, never the file: `crates/pinion-core/src/external.rs`
    holds six, and pairing per file compared one fixture's `count` declaration
    with another fixture's arm.
    """
    core_source = read_core_source()
    groups = variant_groups(core_source)
    vocabulary = declared_as(core_source)
    broken: dict[str, tuple[str, str]] = {}
    unreadable: dict[str, int] = {}
    impls = compared = 0
    for source_path in promise_sources():
        source = source_path.read_text(encoding="utf-8")
        consts = const_items(source)
        for impl_body in impl_bodies(source):
            impls += 1
            here = answered_here(impl_body, groups)
            declared = declared_types(schema_text(impl_body, consts))
            skipped = unknown_tokens(declared, here, vocabulary)
            # ★ `compared` means COMPARED. It used to count every declaration
            # with an arm, skipped ones included, so the round's own ledger said
            # "872 checked ... 0 broken" when 29 of them were never looked at.
            compared += sum(1 for name in declared if name in here) - sum(skipped.values())
            for word, n in skipped.items():
                unreadable[word] = unreadable.get(word, 0) + n
            for name, pair in broken_promises(declared, here, vocabulary).items():
                broken[f"{source_path.relative_to(ROOT)}::{name}"] = pair
    return broken, impls, compared, unreadable


# ── the gate ─────────────────────────────────────────────────────────────────


def check() -> int:
    published, crates = split_census()
    promises, impls, compared, untyped = promise_census()
    now = splits(published)
    new, grown, _ = budget_verdict(now, read_budget())
    shared = sum(1 for by in published.values() if len(by) > 1)
    blind = sum(1 for by in published.values() if "unknown" in by.values())
    stale, unreadable = stale_decodes(published)
    off_spelling, all_tokens, spellings = vocabulary_census()

    # ★★★★★ R2131 — FIRST, because it is the cheapest and it is the one the
    # compiler does not answer until codegen. See `type_tokens_declared` for the
    # measurement: an inline `const {}` block — the shape every surface here
    # uses — is not evaluated by `cargo check` or `cargo clippy`, so between an
    # edit and a full build a synonym is invisible to every gate but this one.
    #
    # ZERO, with no budget, and it is not a judgement call: the legal spellings
    # are read off `SchemaType::as_str`, so this compares the tree against the
    # framework rather than against a list kept here.
    if off_spelling:
        print(
            "read-path-shapes: a schema declares a type word outside the "
            "closed vocabulary",
            file=sys.stderr,
        )
        for where, words in sorted(off_spelling.items()):
            spread = ", ".join(f"{w} x{n}" for w, n in sorted(words.items()))
            print(f"  {where}: {spread}", file=sys.stderr)
        print(
            "read-path-shapes: the vocabulary is `SchemaType` and its spellings\n"
            f"                  are {sorted(spellings)!r}.\n"
            "                  Spell the canonical word. Do NOT add a variant to\n"
            "                  make the word legal unless the framework really\n"
            "                  gained a type -- a synonym is what R2131 removed,\n"
            "                  and the gap it left was hiding two real defects.",
            file=sys.stderr,
        )
        return 1

    # ★ ZERO, not a pin. A split needs two screens to agree on which is wrong;
    # a broken promise is one screen contradicting ITSELF in one file, so
    # there is nothing to negotiate and nothing to schedule.
    if promises:
        print(
            "read-path-shapes: a read path answers two shapes -- its own "
            "declaration and its arm",
            file=sys.stderr,
        )
        for where, (word, shape) in sorted(promises.items()):
            print(f"  {where}: declared `{word}`, answers {shape}", file=sys.stderr)
        print(
            "read-path-shapes: the declaration is a precondition of dispatch\n"
            "                  (R1637), so a wrong type there is the surface\n"
            "                  telling a client something false. Move whichever\n"
            "                  half is wrong; there is no budget for these.",
            file=sys.stderr,
        )
        return 1

    if new or grown:
        print("read-path-shapes: a read path answers two shapes", file=sys.stderr)
        for name in new + grown:
            spread = ", ".join(f"{c}={s}" for c, s in sorted(now[name].items()))
            print(f"  {name}: {spread}", file=sys.stderr)
        print(
            "read-path-shapes: one path name, one `IntrospectValue` variant. A\n"
            "                  caller cannot ask which screen it is talking to\n"
            "                  before reading the answer. If the split is\n"
            "                  deliberate, say why and re-run\n"
            "                  `python3 tools/read_path_shapes.py --write-budget`.",
            file=sys.stderr,
        )
        return 1

    # ★ ZERO as well, and for the third question: a walk that DECODES a read
    # the wire answers as a JSON value. `json.loads(<dict>)` is a `TypeError`,
    # so this is a red demo waiting for whoever runs the sweep next — and the
    # sweep does not gate the push (R835). This is the half that does.
    if stale:
        print(
            "read-path-shapes: a walk decodes a read the wire answers as JSON",
            file=sys.stderr,
        )
        for walk, line, path in stale:
            print(f"  {walk}:{line}: json.loads of `{path}`", file=sys.stderr)
        print(
            "read-path-shapes: `json.loads` of a dict raises TypeError. Drop the\n"
            "                  decode -- the read already hands back a mapping.",
            file=sys.stderr,
        )
        return 1

    # ★★★★★ R2130 — THE PIN ON WHAT THIS CENSUS NEVER LOOKED AT.
    #
    # The three checks above are all about what WAS examined. This one is
    # about the rest, and it exists because the other direction is the one
    # that goes wrong quietly: nothing above notices a declaration LEAVING
    # the comparison. Add one new spelling of a type word and that word's
    # declarations move from `compared` into `skipped`, `broken` stays 0,
    # and every verdict here stays green while coverage falls — which is
    # exactly how the gap that hid R2129's two real mismatches was built.
    #
    # A pin rather than zero, for `budget_verdict`'s reason: the unclassified
    # arms cannot be driven to zero from here at all (following a delegation
    # is what this census structurally cannot do), so demanding zero would
    # make the gate a thing to be bypassed rather than obeyed.
    unseen = unseen_now(untyped, blind)
    appeared, risen, _ = unseen_verdict(unseen, read_unseen())
    if appeared or risen:
        print(
            "read-path-shapes: this census now examines LESS than it is pinned to",
            file=sys.stderr,
        )
        pinned_unseen = read_unseen()
        for kind in appeared + risen:
            was = pinned_unseen.get(kind)
            where = "new" if was is None else f"was {was}"
            print(f"  {kind}: {unseen[kind]} ({where})", file=sys.stderr)
        print(
            "read-path-shapes: `0 broken` is a statement about what is LEFT\n"
            "                  after these, so a rise here weakens every\n"
            "                  verdict above it. Do NOT teach the census a new\n"
            "                  synonym to make this fall -- that blesses the\n"
            "                  split vocabulary and re-hides what it hides.\n"
            "                  Close the vocabulary, or, if the rise is\n"
            "                  deliberate, say why and re-run\n"
            "                  `python3 tools/read_path_shapes.py --write-budget`.",
            file=sys.stderr,
        )
        return 1

    print(
        f"read-path-shapes: {len(published)} read path(s) over {len(crates)} crate(s) "
        f"of {PRODUCT}; {shared} published by more than one, {len(now)} split, "
        f"{blind} with an arm this census cannot classify; "
        f"{compared} declaration(s) over {impls} impl(s) workspace-wide checked "
        f"against their arm, 0 broken "
        f"over the {compared} examined and {sum(unseen.values())} pinned unexamined, "
        f"{sum(untyped.values())} skipped for a type word this census has no "
        f"shape for {dict(sorted(untyped.items()))}; "
        f"{sum(all_tokens.values())} type token(s) declared tree-wide over "
        f"{len(declaration_sources())} source(s), 0 outside the "
        f"{len(spellings)}-word vocabulary; "
        f"{len(read_walks())} walk(s) read for a stale decode, 0 found, "
        f"{unreadable} site(s) with no path this derivation could resolve"
    )
    return 0


def selftest() -> int:
    failed = ran = 0

    def case(name: str, got: object, want: object) -> None:
        nonlocal failed, ran
        ran += 1
        if got != want:
            failed += 1
            print(f"FAIL: {name}: want {want!r}, got {got!r}", file=sys.stderr)

    def require(name: str, held: bool, detail: str = "") -> None:
        nonlocal failed, ran
        ran += 1
        if not held:
            failed += 1
            print(f"FAIL: {name}{': ' + detail if detail else ''}", file=sys.stderr)

    # ★★★★★ The grouping is DERIVED from `pinion-core`, and these fixtures use
    # the real derivation rather than a copy of it. A copy here would be the
    # transcription this round removed, reintroduced in the tests that are
    # supposed to guard it.
    groups = variant_groups(read_core_source())
    case(
        "Raw and Json are ONE shape, because kind() says so",
        (groups.get("Raw"), groups.get("Json")),
        ("json", "json"),
    )
    case(
        "every variant kind() names except Null has a schema token",
        sorted(groups),
        ["Bool", "Float", "Int", "Json", "Raw", "Text"],
    )
    # The join must be total in BOTH directions: a token with no kind word is a
    # word this file invented, and a kind word with no token is a variant the
    # census would grade against nothing.
    # ★★★★★ R2131 — the grouping is now read off `SchemaType::of`, so what is
    # asserted is that the NEW source reproduces what the hand table produced.
    # A derivation that merely returns something is not a replacement; this is
    # what says it returned the same thing.
    case(
        "the grouping derived off SchemaType is the one the hand table had",
        groups,
        {
            "Bool": "bool",
            "Float": "float",
            "Int": "int",
            "Json": "json",
            "Raw": "json",
            "Text": "text",
        },
    )
    case(
        "and Null is excluded BY NAME, not by falling through",
        sorted(_NOT_A_READ_SHAPE),
        ["Null", "Unset"],
    )
    kinds_named = set(groups.values())
    vocabulary = declared_as(read_core_source())
    case(
        "and every declared word maps onto one of them",
        set(vocabulary.values()) - kinds_named,
        set(),
    )
    # ★★★★★ R2131 — the derivation replaces the hand table, so what is asserted
    # is that it produces the SAME join, not that it produces something.
    case(
        "the vocabulary is derived off SchemaType, not written down",
        vocabulary,
        {"json": "json", "string": "text", "bool": "bool", "int": "int", "float": "float"},
    )
    tokens = schema_type_tokens(read_core_source())
    case(
        "every SchemaType variant has exactly one spelling",
        len(set(tokens.values())),
        len(tokens),
    )
    case(
        "and the synonyms R2131 removed are not spellings",
        sorted(set(tokens.values()) & {"boolean", "number", "text", "object", "i32"}),
        [],
    )
    # `null` and the placeholder are real variants with no READ shape, so they
    # must be in the enum and absent from the join. Asserting both directions is
    # what keeps "absent" from meaning "the derivation quietly found nothing".
    case("null is a type", tokens.get("Null"), "null")
    case("but not a read shape", "null" in vocabulary, False)
    case("the placeholder spells empty", tokens.get("Unset"), "")

    case("a json arm", shape_of("Ok(IntrospectValue::Json(spec_json()))", groups), "json")
    case("a text arm", shape_of("text(spec_json().to_string())", groups), "text")
    case(
        "a bool arm",
        shape_of("Ok(IntrospectValue::Bool(state.running.get()))", groups),
        "bool",
    )
    case("an int helper", shape_of("Ok(int(ROWS))", groups), "int")
    case(
        "a delegating arm is unknown", shape_of("read_specification(path)", groups), "unknown"
    )
    case(
        "an arm that can answer two is mixed",
        shape_of("match x { A => text(s), B => Ok(IntrospectValue::Json(v)) }", groups),
        "mixed",
    )
    case(
        "a Raw arm reads as json without this file saying so",
        shape_of("Ok(IntrospectValue::raw(RawJson::new(s)))", groups),
        "json",
    )

    block = (
        '            "spec" => Ok(IntrospectValue::Json(spec_json())),\n'
        '            "zoom" => Ok(IntrospectValue::Int(i64::from(state.zoom.get()))),\n'
        '            "at" => {\n'
        "                let (x, y) = state.at();\n"
        '                text(format!("{x},{y}"))\n'
        "            }\n"
        '            "a" | "b" => Ok(IntrospectValue::Bool(true)),\n'
    )
    case(
        "a multi-line arm travels whole",
        [(n, shape_of(b, groups)) for n, b in arms(block)],
        [(("spec",), "json"), (("zoom",), "int"), (("at",), "text"), (("a", "b"), "bool")],
    )

    src = (
        "impl ExternalIntrospect for X {\n"
        "    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {\n"
        "        match path {\n"
        '            "spec" => Ok(IntrospectValue::Json(v)),\n'
        "            _ => Err(ReadRefusal::UnknownPath),\n"
        "        }\n"
        "    }\n"
        "}\n"
        "fn invoke(&self, path: &str) { match path { \"spec\" => text(s), _ => () } }\n"
    )
    case("only the query match is scanned", len(query_blocks(src)), 1)
    case(
        "and the invoke arm does not reach the census",
        [n for b in query_blocks(src) for n, _ in arms(b)],
        [("spec",)],
    )

    case(
        "a path dep is found, a version dep is not",
        path_deps(
            'hello-node-lab = { path = "../hello-node-lab" }\n'
            'serde_json = { version = "1" }\n'
            "[package]\nname = \"hello-analyzer-shell\"\n"
        ),
        ["hello-node-lab"],
    )
    case(
        "one known shape and one unknown is not a split",
        splits({"spec": {"a": "json", "b": "unknown"}}),
        {},
    )
    case(
        "two known shapes are",
        splits({"spec": {"a": "json", "b": "text"}}),
        {"spec": {"a": "json", "b": "text"}},
    )
    case(
        "a repaired split is offered back",
        budget_verdict({}, {"spec": {"a": "json", "b": "text"}}),
        ([], [], ["spec"]),
    )
    case(
        "a widened split is refused",
        budget_verdict(
            {"said": {"a": "json", "b": "text", "c": "text"}},
            {"said": {"a": "json", "b": "text"}},
        ),
        ([], ["said"], []),
    )
    case(
        "the budget round-trips",
        parse_budget(render_budget({"said": {"a": "json", "b": "text"}})),
        {"said": {"a": "json", "b": "text"}},
    )

    # ── R2130: the pin on what the census cannot examine ──────────────────
    #
    # ★ These four exist because the gate they back is the only one here that
    # fires on coverage FALLING, and a gate with no failing path is not a
    # gate. The middle two are the failing paths, written as the two ways
    # coverage falls: a word nobody had spelled before, and one more site
    # spelling a word already known.
    case(
        "the two unseen kinds are pinned as one table",
        unseen_now({"text": 16, "object": 7}, 16),
        {"type-word:text": 16, "type-word:object": 7, "unclassified-arm": 16},
    )
    case(
        "a type word nobody had spelled before is refused",
        unseen_verdict({"type-word:text": 16, "type-word:bool8": 1}, {"type-word:text": 16}),
        (["type-word:bool8"], [], []),
    )
    case(
        "one more site skipping a known word is refused",
        unseen_verdict({"type-word:text": 17}, {"type-word:text": 16}),
        ([], ["type-word:text"], []),
    )
    case(
        "a word that stopped being skipped is offered back",
        unseen_verdict({"type-word:text": 15}, {"type-word:text": 16}),
        ([], [], ["type-word:text"]),
    )
    case(
        "a word skipped zero times is not a row",
        unseen_now({"text": 0}, 0),
        {},
    )
    case(
        "the unseen pin round-trips",
        parse_unseen(render_unseen({"type-word:text": 16, "unclassified-arm": 16})),
        {"type-word:text": 16, "unclassified-arm": 16},
    )

    case(
        "a declaration is read with its type",
        declared_types(
            '        SchemaField::new("spec", "json"),\n'
            '        SchemaField::action("place", "string"),\n'
        ),
        {"spec": "json"},
    )
    case(
        "a declaration kept is not a promise broken",
        broken_promises({"spec": "json"}, {"spec": "json"}, vocabulary),
        {},
    )
    case(
        "a declaration contradicted is",
        broken_promises({"spec": "string"}, {"spec": "json"}, vocabulary),
        {"spec": ("string", "json")},
    )
    case(
        "an unclassifiable arm breaks no promise",
        broken_promises({"spec": "json"}, {"spec": "unknown"}, vocabulary),
        {},
    )
    case(
        "a declared word this file does not know is left alone",
        broken_promises({"spec": "blob"}, {"spec": "json"}, vocabulary),
        {},
    )
    case(
        "and IS counted, rather than vanishing",
        unknown_tokens({"spec": "blob"}, {"spec": "json"}, vocabulary),
        {"blob": 1},
    )
    case(
        "a Raw answer keeps a `json` promise",
        broken_promises(
            {"frame": "json"},
            {"frame": shape_of("Ok(IntrospectValue::raw(r))", groups)},
            vocabulary,
        ),
        {},
    )
    # ★★★★★ R2131 — THE HOLE BELOW R2130's PIN, as a test. `i32` was not
    # skipped, it was invisible: the old `[a-z]+` type class made the whole
    # `SchemaField::new` fail to match, so the declaration entered neither
    # population. The two cases are the two ways that mattered — a digit in the
    # token, and the empty placeholder.
    case(
        "a type word with a digit is EXTRACTED, so it can be counted",
        declared_types('SchemaField::new("value", "i32")'),
        {"value": "i32"},
    )
    case(
        "and then counted rather than swallowed",
        unknown_tokens({"value": "i32"}, {"value": "int"}, vocabulary),
        {"i32": 1},
    )
    case(
        "the empty placeholder is extracted too",
        declared_types('SchemaField::new("", "")'),
        {"": ""},
    )

    # ★★★★★ R2131 — the tree-wide vocabulary gate, the one that does not wait
    # for codegen. Its failing path is written FIRST, because an assertion with
    # no way to fail is one this project deletes rather than keeps.
    legal = set(schema_type_tokens(read_core_source()).values())
    case(
        "a synonym anywhere is off-vocabulary",
        off_vocabulary(
            type_tokens_declared('SchemaField::new("on", "boolean");'), legal
        ),
        {"boolean": 1},
    )
    case(
        "and the canonical word is not",
        off_vocabulary(type_tokens_declared('SchemaField::new("on", "bool");'), legal),
        {},
    )
    # The argument-INDEX property: `action_with` puts a slice after the token,
    # so a census reading "the literal after the path" would take `form` here.
    case(
        "the token is taken by argument index, not by distance",
        type_tokens_declared(
            'SchemaField::action_with("set", "null", ArgForm::Path, &["x"])'
        ),
        {"null": 1},
    )
    case(
        "`send` declares its RETURN at index 0",
        type_tokens_declared('SchemaField::send("bool")'),
        {"bool": 1},
    )
    case(
        "a SchemaArg token counts too -- one vocabulary, one census",
        type_tokens_declared('SchemaArg::open("n", "number")'),
        {"number": 1},
    )
    case(
        "a comma inside a nested call does not split an argument",
        type_tokens_declared('SchemaField::parametric("c.<r>", "int", &[a(x, y)])'),
        {"int": 1},
    )
    case(
        "a constructor named in a comment is not a declaration",
        type_tokens_declared('// SchemaField::new("on", "boolean")\n'),
        {},
    )
    case(
        "nor is one inside a string",
        type_tokens_declared('let s = "SchemaField::new(\\"on\\", \\"boolean\\")";\n'),
        {},
    )
    case(
        "a token passed as a const is left to its own definition",
        type_tokens_declared('SchemaField::new("on", WORD)'),
        {},
    )

    # ★★★★★ The WRAPPED decode — the shape a grep cannot see and the reason
    # this derivation is structural. The path literal and the `json.loads` are
    # on different lines and in different functions.
    WRAPPED = (
        "def q(app, path):\n"
        '    return app.query(f"{EXT}/{path}")\n'
        "def qj(app, path):\n"
        "    return json.loads(q(app, path))\n"
        "def use(app):\n"
        '    return qj(app, "spec")["enum_key"]\n'
    )
    case(
        "a decode wrapped in a helper is found, with the CALLER's path",
        decoding_sites(WRAPPED),
        ([(6, "spec")], 0),
    )
    case(
        "a direct decode is found too",
        decoding_sites('def use(tf):\n    return json.loads(tf.query(f"{EXT}/spec"))\n'),
        ([(2, "spec")], 0),
    )
    case(
        "a decode of something that is not a wire read is not a site",
        decoding_sites("def use(p):\n    return json.loads(p.read_text())\n"),
        ([], 0),
    )
    # ★ The blind spot has to be REACHABLE or the number it reports is a
    # decoration. A parametric decoder called with a variable is the shape.
    case(
        "a wrapped decode whose caller passes a variable is UNRESOLVED",
        decoding_sites(WRAPPED.replace('qj(app, "spec")', "qj(app, path)")),
        ([], 1),
    )
    case(
        "a decode at module level with no literal is unresolved too",
        decoding_sites("def q(app, p):\n    return app.query(p)\n" "x = json.loads(q(a, p))\n"),
        ([], 1),
    )
    case(
        "a decode is stale when the screen it drives answers json",
        decode_is_stale({"spec": {"lab": "json", "shell": "unknown"}}, {"lab"}, "spec"),
        True,
    )
    case(
        "and is not when that screen still answers text",
        decode_is_stale({"said": {"lab": "json", "sessions": "text"}}, {"sessions"}, "said"),
        False,
    )
    case(
        "a walk driving only an unclassified screen is left alone",
        decode_is_stale({"spec": {"shell": "unknown"}}, {"shell"}, "spec"),
        False,
    )

    # ★★★★★ THE TWO CASES THIS TOOL'S OWN FIRST DRAFT GOT WRONG, kept because a
    # scanner defect here does not look like a defect — it looks like a screen
    # publishing fewer paths than it does.
    two = (
        '            "a" => {\n'
        "                let x = 1;\n"
        "            }\n"
        '            "b" => Ok(IntrospectValue::Json(v)),\n'
    )
    case(
        "a block-bodied arm does not swallow the arm after it",
        [n for n, _ in arms(two)],
        [("a",), ("b",)],
    )
    case(
        "a comment naming another variant is not read",
        shape_of("// answers Ok(IntrospectValue::Json(v)) elsewhere\n text(s)", groups),
        "text",
    )
    case(
        "a comma inside a string does not close an arm early",
        arms('            "a" => text(format!("{x},{y}")),\n            "b" => x,\n'),
        [(("a",), 'text(format!("{x},{y}"))'), (("b",), "x")],
    )
    case("a char literal holding a brace is not code", code_mask("'{'")[1], False)
    case("a lifetime is code", code_mask("&'a str")[1], True)
    # ★★★★★ These two are the cases a counterfactual had to ASK for. The first
    # draft of the run above broke the mask inside the arm scanner and every
    # assertion here stayed green, because no fixture held an UNBALANCED
    # bracket where the mask is what hides it. A guard nothing can fail is not
    # a guard, and the fixtures — not the assertion — were the thing missing.
    case(
        "a bracket inside a comment opens no depth",
        [
            n
            for n, _ in arms(
                '            "a" => Ok(IntrospectValue::Json(\n'
                "                // the wire's shape (see the arm above\n"
                "                v,\n"
                "            )),\n"
                '            "b" => y,\n'
            )
        ],
        [("a",), ("b",)],
    )
    case(
        "a bracket inside a string opens no depth",
        [n for n, _ in arms('            "a" => text("("),\n            "b" => y,\n')],
        [("a",), ("b",)],
    )

    # ★★★★★ The ORACLES, exercised by name against the real tree — a pure rule
    # tested only on fixtures is a rule nobody has aimed at anything
    # (`tools/oracle_census.py` is the standing count of this).
    crates = product_crates()
    published, _ = split_census()
    promises, impls, compared, unreadable = promise_census()
    case(f"{PRODUCT} is the root of its own product", crates[0], PRODUCT)
    require("no screen contradicts its own declaration", not promises, f"{promises}")
    require("the promise census reads the whole workspace, not the product", impls > len(crates))
    require("and it compares declarations, rather than finding none", compared > 100)
    # ★★★★★ R2131 FLIPPED THIS ASSERTION, AND THE FLIP IS THE ROUND.
    #
    # It used to require that the skipped words be NON-EMPTY — "asserted to be
    # visible, not asserted to be zero" — because the vocabulary had synonyms
    # and a census demanding zero would have been demanding the defect be
    # hidden. That was right while the only way to reach zero was to teach this
    # file the synonyms. R2131 reached zero the other way: `SchemaField.ty` is a
    # closed `SchemaType`, so the synonyms are not spellable and there is
    # nothing left to skip.
    #
    # ⚠ The failing path did not move to "nothing can fail". It moved UP, to
    # the pure cases above, which hold that an unknown word is still extracted
    # and still counted — `blob` and `i32` both. If this ever rises again it is
    # a real widening of the vocabulary, and R2130's pin refuses it first.
    require(
        "no declaration is skipped for a type word: the vocabulary is closed",
        not unreadable,
        f"{unreadable}",
    )
    require(
        "and none of them is a word the census claims to know",
        not (set(unreadable) & set(vocabulary)),
        f"{sorted(set(unreadable) & set(vocabulary))}",
    )
    # ★★★★★ R2131 — the ORACLE half, aimed at the real tree. R2130.1 was opened
    # by exactly this gap: a pure rule had cases and its oracle had none, so
    # nothing said whether the rule was ever pointed at anything.
    off_spelling, all_tokens, spellings = vocabulary_census()
    require(
        "no source declares a type word outside the vocabulary",
        not off_spelling,
        f"{off_spelling}",
    )
    require(
        "and the census actually read the tree, rather than finding nothing",
        sum(all_tokens.values()) > 1000 and len(declaration_sources()) > 50,
        f"{sum(all_tokens.values())} token(s) over {len(declaration_sources())} source(s)",
    )
    case(
        "the spellings come off SchemaType, so the gate cannot drift from it",
        spellings,
        set(schema_type_tokens(read_core_source()).values()),
    )
    walks_stale, walks_blind = stale_decodes(published)
    require("no walk decodes a read the wire answers as JSON", not walks_stale)
    # ★ The blind spot is asserted, not merely printed: the round that measured
    # it found ZERO, and a rise means a decode this derivation stopped reading.
    require("every wire decode resolves to a path", walks_blind == 0, f"{walks_blind}")
    require(f"{PRODUCT} reaches a screen crate by path", len(crates) > 1)
    require(
        f"read_manifest({PRODUCT}) yields a path dep",
        bool(path_deps(read_manifest(PRODUCT))),
    )
    require(
        f"read_sources({PRODUCT}) finds an introspect impl",
        any("ExternalIntrospect" in s for s in read_sources(PRODUCT)),
    )
    # Non-vacuity: a census that found nothing would agree with any budget.
    require("the census reads shared names at all", sum(1 for v in published.values() if len(v) > 1) > 1)
    stale = sorted(set(read_budget()) - set(splits(published)))
    require(
        "the budget pins no split that is already repaired",
        not stale,
        f"{stale} (run --write-budget)",
    )

    # ★★★★★ R2130.1 — THE UNSEEN PIN'S ORACLE, AIMED AT THE REAL TREE.
    #
    # `tools/oracle_census.py` refused the push that added this pin, and it
    # was right: `unseen_verdict` had six fixture cases and `read_unseen` —
    # the oracle that gives it its world — was called by nothing. A pure
    # rule and its oracle are TWO gates, and the one that gets remembered is
    # the pure one, because it is the one that is pleasant to test.
    #
    # Mirrors the budget's staleness require one line up, and for its reason:
    # a pin carrying a count HIGHER than reality means someone repaired
    # something and did not re-run `--write-budget`, so the gate is now
    # measuring against a number that no longer describes the tree. That is
    # the direction this pin fails in silently — a too-high pin never
    # refuses anything.
    pinned_unseen = read_unseen()
    blind_now = sum(1 for by in published.values() if "unknown" in by.values())
    # Non-vacuity first: an empty pin would agree with any tree at all.
    require(
        "the unseen pin describes something",
        sum(pinned_unseen.values()) > 0,
        f"{pinned_unseen} (run --write-budget)",
    )
    _, _, dropped = unseen_verdict(unseen_now(unreadable, blind_now), pinned_unseen)
    require(
        "the unseen pin counts nothing this census now examines",
        not dropped,
        f"{dropped} (run --write-budget)",
    )

    print(f"read_path_shapes selftest: {ran - failed} of {ran} checks OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="the ratchet (default)")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--list", action="store_true", help="every shared name")
    parser.add_argument("--write-budget", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.write_budget:
        published = split_census()[0]
        now = splits(published)
        BUDGET.write_text(render_budget(now), encoding="utf-8")
        print(f"wrote {BUDGET.relative_to(ROOT)}: {len(now)} split(s)")
        # R2130 — BOTH pins, in one command. Two writers would let a round
        # refresh the splits and leave the unseen pin stale, and a stale pin
        # is worse than none: it reads as a measurement.
        _, _, _, untyped_words = promise_census()
        unclassified = sum(
            1 for by in published.values() if "unknown" in by.values()
        )
        unseen = unseen_now(untyped_words, unclassified)
        UNSEEN.write_text(render_unseen(unseen), encoding="utf-8")
        print(
            f"wrote {UNSEEN.relative_to(ROOT)}: "
            f"{sum(unseen.values())} declaration(s) this census cannot examine"
        )
        return 0
    if args.list:
        published, _ = split_census()
        promises_seen, _, _, unread_words = promise_census()
        for where, (word, shape) in sorted(promises_seen.items()):
            print(f"PROMISE {where}\tdeclared={word}, answers={shape}")
        for word, n in sorted(unread_words.items()):
            print(f"UNTYPED declared `{word}` x{n}\tno shape for this word")
        walk_stale, walk_blind = stale_decodes(published)
        for walk, line, path in walk_stale:
            print(f"DECODE  {walk}:{line}\tjson.loads of {path}")
        print(f"UNREAD  {walk_blind} decode site(s) with no resolvable path")
        split_names = splits(published)
        for name, by_crate in sorted(published.items()):
            if len(by_crate) < 2:
                continue
            mark = "SPLIT" if name in split_names else "     "
            spread = ", ".join(f"{c}={s}" for c, s in sorted(by_crate.items()))
            print(f"{mark} {name}\t{spread}")
        return 0
    return check()


if __name__ == "__main__":
    sys.exit(main())
