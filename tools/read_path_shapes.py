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

# THREE checks over THREE populations, and only one of them has a budget

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

#: The root of the assembled product. Its path dependencies are the population;
#: this one name is the only thing about the population written down here.
PRODUCT = "hello-analyzer-shell"

#: An arm's text is searched for these, longest spelling first so that
#: `IntrospectValue::Int` cannot be read out of `IntrospectValue::Integer`-like
#: neighbours by prefix. The right-hand side is what a caller must be ready for.
_CONSTRUCTORS: tuple[tuple[str, str], ...] = (
    ("IntrospectValue::Json", "json"),
    ("IntrospectValue::Bool", "bool"),
    ("IntrospectValue::Float", "float"),
    ("IntrospectValue::Text", "text"),
    ("IntrospectValue::Int", "int"),
    # ★★★★★ `Raw` IS json, and calling it anything else was this tool's own
    # first defect. `IntrospectValue::kind()` in `pinion-core` answers
    # `Json(_) | Raw(_) => "json"` — a `Raw` carries JSON text the producer
    # already had and reaches the wire verbatim. Classifying it as a seventh
    # word made two correct surfaces (`hello-encoded-answer`'s `frame`, a
    # dispatch fixture's `raw`) read as broken promises. A census that invents
    # a vocabulary the framework does not have reports its own invention.
    ("IntrospectValue::Raw", "json"),
    ("IntrospectValue::raw", "json"),
)

#: Local one-word constructors the screens in this product wrap the above in.
#: Each is a named helper whose body is `IntrospectValue::<V>`; they are listed
#: because a text census cannot follow a call, which is the `unknown` blind
#: spot the module docstring states.
_HELPERS: tuple[tuple[str, str], ...] = (
    ("text", "text"),
    ("int", "int"),
    ("plain", "text"),
)

#: The word a `SchemaField` declares, and the arm shape that keeps its promise.
#: Only the words this product actually declares for READS are listed; a word
#: that is not here is not compared, because inventing the mapping for one
#: would be this file deciding the framework's vocabulary.
_DECLARED_AS: dict[str, str] = {
    "json": "json",
    "string": "text",
    "bool": "bool",
    "int": "int",
    "float": "float",
}

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


def shape_of(body: str) -> str:
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
    found = {shape for spelling, shape in _CONSTRUCTORS if spelling in code}
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
    """
    return {
        m.group(1): m.group(2)
        for m in re.finditer(
            r"SchemaField::new\(\s*\"([A-Za-z_0-9]+)\"\s*,\s*\"([a-z]+)\"", source
        )
    }


def broken_promises(
    declared: dict[str, str], answered: dict[str, str]
) -> dict[str, tuple[str, str]]:
    """`name -> (declared word, answered shape)` where the two disagree.

    A shape the census could not classify is not a disagreement, for the reason
    it is not a split: `unknown` is this census's limit, not the screen's
    defect. A declared word with no entry in `_DECLARED_AS` is likewise left
    alone — the vocabulary is the framework's and may grow.
    """
    out: dict[str, tuple[str, str]] = {}
    for name, word in declared.items():
        shape = answered.get(name)
        want = _DECLARED_AS.get(word)
        if shape in (None, "unknown", "mixed") or want is None:
            continue
        if want != shape:
            out[name] = (word, shape)
    return out


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


def read_walks() -> list[Path]:
    """Every demo walk, in a stable order."""
    return sorted((ROOT / "tools" / "demos").glob("*.py"))


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


def answered_here(impl_body: str) -> dict[str, str]:
    """The shape each read path is answered with, inside ONE impl."""
    out: dict[str, str] = {}
    for block in query_blocks(impl_body):
        for names, body in arms(block):
            shape = shape_of(body)
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
    published: dict[str, dict[str, str]] = {}
    for crate in crates:
        for source in read_sources(crate):
            if "ExternalIntrospect" not in source:
                continue
            for impl_body in impl_bodies(source):
                for name, shape in answered_here(impl_body).items():
                    was = published.get(name, {}).get(crate)
                    published.setdefault(name, {})[crate] = (
                        shape if was in (None, shape, "unknown") else "mixed"
                    )
    return published, crates


def promise_census() -> tuple[dict[str, tuple[str, str]], int, int]:
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
    broken: dict[str, tuple[str, str]] = {}
    impls = compared = 0
    for source_path in promise_sources():
        source = source_path.read_text(encoding="utf-8")
        consts = const_items(source)
        for impl_body in impl_bodies(source):
            impls += 1
            here = answered_here(impl_body)
            declared = declared_types(schema_text(impl_body, consts))
            compared += sum(1 for name in declared if name in here)
            for name, pair in broken_promises(declared, here).items():
                broken[f"{source_path.relative_to(ROOT)}::{name}"] = pair
    return broken, impls, compared


# ── the gate ─────────────────────────────────────────────────────────────────


def check() -> int:
    published, crates = split_census()
    promises, impls, compared = promise_census()
    now = splits(published)
    new, grown, _ = budget_verdict(now, read_budget())
    shared = sum(1 for by in published.values() if len(by) > 1)
    blind = sum(1 for by in published.values() if "unknown" in by.values())
    stale, unreadable = stale_decodes(published)

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

    print(
        f"read-path-shapes: {len(published)} read path(s) over {len(crates)} crate(s) "
        f"of {PRODUCT}; {shared} published by more than one, {len(now)} split, "
        f"{blind} with an arm this census cannot classify; "
        f"{compared} declaration(s) over {impls} impl(s) workspace-wide checked "
        f"against their arm, 0 broken; "
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

    case("a json arm", shape_of("Ok(IntrospectValue::Json(spec_json()))"), "json")
    case("a text arm", shape_of("text(spec_json().to_string())"), "text")
    case("a bool arm", shape_of("Ok(IntrospectValue::Bool(state.running.get()))"), "bool")
    case("an int helper", shape_of("Ok(int(ROWS))"), "int")
    case("a delegating arm is unknown", shape_of("read_specification(path)"), "unknown")
    case(
        "an arm that can answer two is mixed",
        shape_of("match x { A => text(s), B => Ok(IntrospectValue::Json(v)) }"),
        "mixed",
    )
    # ★ `IntrospectValue::Int` must not be read out of a longer spelling that
    # merely starts the same way. The tuple is ordered for this; the assertion
    # is what would notice the order being lost.
    case(
        "the json spelling wins over a bare Int prefix",
        shape_of("Ok(IntrospectValue::Json(serde_json::json!({ \"n\": 1 })))"),
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
        [(n, shape_of(b)) for n, b in arms(block)],
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
        broken_promises({"spec": "json"}, {"spec": "json"}),
        {},
    )
    case(
        "a declaration contradicted is",
        broken_promises({"spec": "string"}, {"spec": "json"}),
        {"spec": ("string", "json")},
    )
    case(
        "an unclassifiable arm breaks no promise",
        broken_promises({"spec": "json"}, {"spec": "unknown"}),
        {},
    )
    case(
        "a declared word this file does not know is left alone",
        broken_promises({"spec": "blob"}, {"spec": "json"}),
        {},
    )
    case(
        "a Raw answer keeps a `json` promise",
        broken_promises({"frame": "json"}, {"frame": shape_of("Ok(IntrospectValue::raw(r))")}),
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
        shape_of("// answers Ok(IntrospectValue::Json(v)) elsewhere\n text(s)"),
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
    promises, impls, compared = promise_census()
    case(f"{PRODUCT} is the root of its own product", crates[0], PRODUCT)
    require("no screen contradicts its own declaration", not promises, f"{promises}")
    require("the promise census reads the whole workspace, not the product", impls > len(crates))
    require("and it compares declarations, rather than finding none", compared > 100)
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
        now = splits(split_census()[0])  # the budget pins SPLITS; nothing else
        BUDGET.write_text(render_budget(now), encoding="utf-8")
        print(f"wrote {BUDGET.relative_to(ROOT)}: {len(now)} split(s)")
        return 0
    if args.list:
        published, _ = split_census()
        for where, (word, shape) in sorted(promise_census()[0].items()):
            print(f"PROMISE {where}\tdeclared={word}, answers={shape}")
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
