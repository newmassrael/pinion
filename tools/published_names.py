#!/usr/bin/env python3
"""R2107 — **no published object names a key twice**, over the whole tree.

    python3 tools/published_names.py            # the ratchet (pre-push)
    python3 tools/published_names.py --selftest # its own tests
    python3 tools/published_names.py --list     # the population, per file

# What this is, and why R2105's gate is not enough

`serde_json::json!` accepts two identical keys **without a word** and keeps the
last, so the earlier value never reaches a reader. R2105 paid a whole sweep for
that class: an address roster published as `spec.inspector` collided with the
pane's pinned conformance document under the same name, the roster was dropped
at build time, twelve converted walks failed with `KeyError('seats')`, and
nothing in the failure named the key or the collision.

★★★★★ **The defect is invisible in the VALUE.** By the time there is a
`serde_json::Value` the two keys have already collapsed into one, so no
assertion about the wire — and none about the running screen — can reach it. It
exists only in the SOURCE. That is why this tool reads TEXT where nearly every
other census in this repository reads structure, and the fact is written here
rather than left for the next reader to rediscover as an inconsistency.

R2105's gate is a scan of **one function's outer object** in **one crate**:
it `include_str!`s `lib.rs`, splits on `fn spec_json`, and counts keys at the
depth that function's own `json!` sits at. Measured at R2107, that is **1 of
664** object literals in this tree. The other 663 had no reader at all.

# ★★ The generalisation that looks obvious is WRONG, and it cries wolf

R2105's scan identifies an object BY ITS DEPTH, which is sound there because
exactly one object sits at the depth it looks at. Generalising that — "count
keys per depth" — was written first at R2107 and answered **111 duplicates**,
every one of them false: a roster is an ARRAY of objects, so
`[{"key": a, "title": b}, {"key": c, "title": d}]` puts two legitimate `key`s
at one depth. Sibling objects are not one object.

⇒ a key belongs to the innermost **brace**, not to a depth. [`duplicate_keys`]
keeps a stack: `{` opens a fresh key namespace, `}` closes it, and `[` and `(`
are pushed as NON-objects so a key can never be attributed to them. The
selftest pins the array-of-rosters case, because that is the one the obvious
rule gets wrong.

# ⚠ What this deliberately does NOT refuse

Two DIFFERENT surfaces sharing a name. Measured at R2107: the node lab
publishes 35 top-level specification keys and 84 introspect read paths, and
**eight names are on both** (`nodes`, `graph`, `roles`, `zoom`, `frames`,
`links`, `observed`, `selected_link`); the assembled shell has seven more.
Every one is a convention rather than a collision — `spec.nodes` is the card
roster the specification DECLARES and `/external/nodes` is what is on the
canvas NOW, two questions about one subject, and sharing the word is what makes
them recognisable as a pair. Nothing collapses: they are different addresses.

R2106 declined to publish an address roster as `palette` because that word
already names a read path meaning *the live colour register*, and that judgment
stands — but it is a judgment about two DIFFERENT things wearing one word, not
about a value being lost. A gate here would refuse fifteen sound declarations,
so this tool refuses the collapsing class only, and the distinction is written
down so the wider gate is not proposed again.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ROOTS = ("crates", "examples")

#: Where a `json!` object literal begins. The macro may be written
#: `json!({`, `serde_json::json!({` or with spaces between the parts.
MACRO = re.compile(r"json!\s*\(\s*\{")


def duplicate_keys(text: str, start: int) -> list[tuple[int, str]]:
    """Every key repeated inside ONE brace of the object literal at `start`.

    `start` is the index just past the macro's opening brace. The answer is
    `(offset, key)` pairs, so a caller can turn an offset into a line without
    this rule knowing what a file is.

    ★ Pure: it takes the text and an offset and reads nothing else, which is
    what lets the selftest drive the cases below without a tree. Its ORACLE is
    [`scan`], and `tools/oracle_census.py` is what holds that oracle to being
    exercised — the pairing this repository requires of every pure rule.

    ⚠ Strings are skipped whole, honouring backslash escapes, so a `"` inside a
    value cannot desynchronise the brace stack. A string counts as a KEY only
    when a colon follows it and that colon is not the head of a `::` path, which
    is how `serde_json::json!` itself appears inside a nested value.
    """
    stack: list[set[str] | None] = [set()]
    found: list[tuple[int, str]] = []
    i = start
    while i < len(text) and stack:
        ch = text[i]
        if ch == '"':
            j = i + 1
            while j < len(text) and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            after = text[j + 1 :].lstrip()
            if after.startswith(":") and not after.startswith("::"):
                key = text[i + 1 : j]
                top = stack[-1]
                if top is not None:
                    if key in top:
                        found.append((i, key))
                    top.add(key)
            i = j + 1
            continue
        if ch == "{":
            stack.append(set())
        elif ch in "[(":
            stack.append(None)
        elif ch in "]})":
            stack.pop()
        i += 1
    return found


def source_text(path: Path) -> str:
    """The text of one scanned file — **the oracle for [`duplicate_keys`]**.

    ★★★★★ R2107 — a NAMED function rather than an inline `path.read_text(…)`,
    and the difference is whether `tools/oracle_census.py` can see the pair at
    all. That census forms a pair when a pure rule is called with a local
    assigned from an IMPURE FUNCTION, and it reports the pair satisfied when the
    tool's own `selftest` calls that function BY NAME. An inline method call has
    no name, so no pair forms, nothing is reported missing, and the rule's
    oracle is exactly the untested second gate
    `debt-a-pure-rules-oracle-is-a-second-gate-nobody-tests` was opened for.

    ⚠ Measured on this tool's own first draft, and the tell is visible in the
    census's summary rather than in a failure: tools went 26 → 27 while PAIRS
    stayed at 37. A count that grows in one column and not the other is the
    shape of a rule whose oracle nobody can name.
    """
    return path.read_text(encoding="utf-8", errors="replace")


def sources() -> list[Path]:
    """Every Rust file under the workspace roots that mentions the macro."""
    out: list[Path] = []
    for root in ROOTS:
        for path in sorted((REPO / root).rglob("*.rs")):
            if "json!" in source_text(path):
                out.append(path)
    return out


def scan() -> tuple[int, int, list[tuple[str, int, str]]]:
    """`(files, object literals, findings)` over the whole tree.

    The oracle for [`duplicate_keys`]: it is what decides WHICH text the pure
    rule is asked about, and a fixture cannot reach that decision.
    """
    files = 0
    objects = 0
    findings: list[tuple[str, int, str]] = []
    for path in sources():
        text = source_text(path)
        files += 1
        for m in MACRO.finditer(text):
            objects += 1
            for at, key in duplicate_keys(text, m.end()):
                line = text.count("\n", 0, at) + 1
                findings.append((str(path.relative_to(REPO)), line, key))
    return files, objects, findings


def publishers() -> list[Path]:
    """Every file that builds a screen's published specification.

    ★★★★★ R2107 — the population's own anti-vacuity guard, and DERIVED rather
    than a list of screen names. R2053's finding is that a gate whose
    population is hand-written carries this class one level up: a screen added
    later would simply not be looked at, and nothing would say so.

    A screen publishes its specification from `fn spec_json`, which is the
    surface R2105's collision happened on and the one an agent reads first. If
    any such file is missing from [`sources`], this census has stopped covering
    a screen — including the assembled shell, which mounts the others and
    publishes a specification of its own.
    """
    out: list[Path] = []
    for root in ROOTS:
        for path in sorted((REPO / root).rglob("*.rs")):
            if "fn spec_json" in source_text(path):
                out.append(path)
    return out


def check() -> int:
    files, objects, findings = scan()
    # ★ A screen that publishes a specification and is NOT in the population is
    # a hole this census cannot report as a duplicate, because it never looks.
    covered = set(sources())
    uncovered = [p for p in publishers() if p not in covered]
    if uncovered:
        print("published names: these screens publish a specification and are not scanned:")
        for path in uncovered:
            print(f"  {path.relative_to(REPO)}")
        return 1
    # ★ The population is REPORTED whether or not anything is wrong. A ratchet
    # that speaks only when it fires cannot be told from one that quietly
    # stopped looking — this repository's standing rule, and the reason
    # `painted_addresses` prints its size every push.
    if not findings:
        print(
            f"published names: {objects} json object(s) in {files} file(s), "
            f"{len(publishers())} of them publishing a specification, "
            "no key named twice"
        )
        return 0
    print("published names: an object names a key twice; `json!` keeps the LAST")
    for path, line, key in findings:
        print(f"  {path}:{line}  {key!r}")
    print(
        "published names: the earlier value never reaches a reader, and no "
        "assertion about the wire can see it — the two keys have already "
        "collapsed by the time there is a `Value`. Rename one."
    )
    return 1


def listing() -> int:
    files, objects, _ = scan()
    for path in sources():
        text = source_text(path)
        n = len(MACRO.findall(text))
        print(f"{n:5d}  {path.relative_to(REPO)}")
    print(f"published names: {objects} object literal(s) in {files} file(s)")
    return 0


def selftest() -> int:
    """Cases whose answers are known by hand, plus the tree-derived guard.

    ⚠ FIXTURES, not this tree: a case pinned to today's `examples/` rots the way
    the prose it replaced rotted, which `painted_addresses` recorded at R2103.
    What does not rot is the distinction each case is about.
    """
    failures: list[str] = []

    def case(name: str, body: str, want: list[str]) -> None:
        m = MACRO.search(body)
        if m is None:
            failures.append(f"{name}: the fixture has no macro to scan")
            return
        got = [k for _, k in duplicate_keys(body, m.end())]
        if got != want:
            failures.append(f"{name}: answered {got!r}, expected {want!r}")

    case("clean object", 'json!({ "a": 1, "b": 2 })', [])
    case("a key twice", 'json!({ "a": 1, "b": 2, "a": 3 })', ["a"])
    # ★★★★★ THE CASE THE OBVIOUS RULE GETS WRONG. A roster is an array of
    # objects and every row carries the same keys; counting per DEPTH calls
    # that a duplicate, which is how the first draft of this tool answered 111
    # findings of which zero were real.
    case(
        "sibling objects in an array",
        'json!({ "rows": [ { "key": 1, "title": 2 }, { "key": 3, "title": 4 } ] })',
        [],
    )
    # A nested object may reuse a name its parent used — `tag` occurs in most of
    # this tree's rosters at more than one level.
    case(
        "a nested object reuses the parent's name",
        'json!({ "tag": "a", "seat": { "tag": "b" } })',
        [],
    )
    # ...and a duplicate INSIDE the nested object is still one.
    case(
        "a nested object names its own key twice",
        'json!({ "seat": { "tag": "a", "tag": "b" } })',
        ["tag"],
    )
    # A `::` path inside a value is not a key, or every `serde_json::json!`
    # nested in a value would be counted as one.
    case(
        "a path in a value is not a key",
        'json!({ "a": serde_json::json!({ "b": 1 }) })',
        [],
    )
    # A quote inside a value must not desynchronise the stack; without escape
    # handling the brace after it is read inside a string and the scan ends
    # early, so the later duplicate goes unseen.
    case(
        "an escaped quote in a value",
        r'json!({ "said": "he said \" then {", "a": 1, "a": 2 })',
        ["a"],
    )
    # A key repeated three times is two findings, so a message can say how many.
    case("three times", 'json!({ "a": 1, "a": 2, "a": 3 })', ["a", "a"])

    # ★★★★★ The ORACLE, exercised — `oracle_census.py` (R2028) exists because a
    # pure rule's fixtures leave the thing that CHOOSES its input untested. A
    # `sources()` that read nothing would leave every case above green while the
    # ratchet reported the tree clean.
    files, objects, _ = scan()
    if files < 20:
        failures.append(f"the oracle found only {files} file(s) with the macro")
    if objects < 200:
        failures.append(f"the oracle found only {objects} object literal(s)")
    # And the population is DERIVED a second way, so a broken `MACRO` pattern
    # cannot agree with itself: count the bare textual occurrences of the macro
    # name across the same files and require the parsed count to reach most of
    # them. Not equality — `json!` also appears in prose and in `json!([…])`
    # array literals, which this tool does not scan.
    bare = sum(source_text(p).count("json!") for p in sources())
    if objects > bare:
        failures.append(f"parsed {objects} objects from {bare} mentions of the macro")
    # ★ And every specification-publishing screen is inside the population, so
    # the guard `check` applies is one the selftest has also exercised.
    missing = [p for p in publishers() if p not in set(sources())]
    if missing:
        failures.append(f"{len(missing)} screen(s) publish a specification unscanned")

    for line in failures:
        print(f"FAIL: {line}")
    total = 8 + 4
    print(f"published_names selftest: {total - len(failures)} of {total} cases OK")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--list", action="store_true", dest="listing")
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.listing:
        return listing()
    return check()


if __name__ == "__main__":
    sys.exit(main())
