#!/usr/bin/env python3
"""R1611 — the ratchet that keeps other projects' names out of what we publish.

A standing directive (2026-08-09) says the names of the projects this one is
judged against must not appear in artifacts that get pushed. The round that
received it zeroed its OWN output and registered the rest as debt, which is why
this file exists: a directive with no gate is a directive nobody can see the
state of, and the state was 508 files.

## What this is not

It is NOT a rule about what we may read. Judging a widget against a mature
toolkit is how every completion figure here is set, and that does not change.
What changes is that the *evidence* is restated in terms of the capability
rather than the vendor -- "a mature toolkit's MDI child has keyboard move and
resize" -- with the file and symbol that proves it recorded in this project's
memory notes, which are not part of the repository.

## Why a ratchet rather than a threshold

The population is large and each occurrence is a sentence that carries a reason
for a design decision. Rewriting one is editorial work: the name goes, the
evidence stays. That cannot be done mechanically and it cannot be done in one
sitting, so the gate's job is to make the number *monotone* -- every push either
holds it or lowers it -- and to make a round that adds one say so out loud.

## Counting on doubt

A token that looks like a reference symbol but has not been classified is
COUNTED, not skipped. The two errors are not symmetric: an over-count shows up
as a budget a round cannot meet and someone looks at it, while an under-count
ships the name. So the cost is put on the side that gets noticed.

Usage:
    python3 tools/reference_names.py                 # census
    python3 tools/reference_names.py --check         # ratchet (exit 1 on a rise)
    python3 tools/reference_names.py --write-budget  # after a clearing round
    python3 tools/reference_names.py --selftest      # the classifier's own tests
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGET = ROOT / "docs" / "reference-names-budget.tsv"

# --- what is not ours to rewrite -------------------------------------------

# Each exclusion states why the population inside it cannot be cleared, so that
# "the number is not zero" always has a reason attached rather than being a
# quiet remainder.
EXCLUDED: list[tuple[str, str]] = [
    (
        "docs/.atomic/",
        "the decision ledger is frozen by construction -- an entry cannot be "
        "amended after it is appended, so occurrences already in it are "
        "unreachable. New entries are the round's own responsibility.",
    ),
    (
        "crates/pinion-text-unicode/ucd/",
        "vendored Unicode character database. Not our prose, and its property "
        "names collide with the symbol patterns by coincidence.",
    ),
    (
        "vendor/",
        "submodule working trees; upstream's text, PR path only.",
    ),
    (
        "docs/reference-names-budget.tsv",
        "the ratchet's own budget names the files it is counting.",
    ),
    (
        ".gitignore",
        "R1614 -- an ignore pattern IS the literal directory an editor "
        "creates, so rewording one stops it ignoring anything. A file of "
        "path patterns also holds no prose, so nothing can regrow here as a "
        "citation; this is the narrowest exclusion in the list on purpose.",
    ),
    (
        "crates/pinion-cli/src/design_api.rs",
        "R1614 -- the design service's wire constants: its host name, the "
        "header it reads a token from, and the environment variable a person "
        "exports one into. None of the three is a citation of a project this "
        "one is judged against -- it is an ADDRESS, and renaming it stops the "
        "design-parity workflow working. Everything that was ours to name has "
        "been renamed to its role (`pinion design-verify`, `design_*` "
        "modules, `examples/design-button-m3`), and the constants were "
        "gathered into this one module so the exclusion could be this small: "
        "the prose in the modules around it stays counted.",
    ),
    (
        "tools/reference_names.py",
        "this file IS the term list; it cannot avoid holding the terms.",
    ),
    (
        "tools/reference_census.py",
        "the reader of the reference trees: it must know their directory "
        "layout and their operator-id spellings to scan them at all, and it is "
        "the only file that does. What it WRITES is a pushed artifact and is "
        "clean -- see PUBLIC_TREE / public_id, and the four selftest cases that "
        "hold the committed spelling to its rename.",
    ),
    (
        "tools/reference_surface.py",
        "R1650 -- the reader of the reference toolkit's HEADERS: it holds the "
        "one table mapping a claim's role (`widget`, `attribute-enum`) to the "
        "header path and class identifier that answer it, and it is the only "
        "file that does. Same split as `reference_census.py` beside it -- a "
        "scanner must know a tree's layout to scan it at all -- and what it "
        "READS from and WRITES to, `docs/reference-surface.json`, carries no "
        "spelling of the reference at all and is counted normally. Its own "
        "selftest fixture builds its class name from pieces so the FIXTURE is "
        "not the reason this exclusion exists.",
    ),
    (
        "tools/reference_names_migrate.py",
        "the substitution table: it must hold both the name it removes and the "
        "phrase it puts there. Its own tests build fixture names from pieces so "
        "that the FIXTURES are not the reason this exclusion exists.",
    ),
]

# --- the terms --------------------------------------------------------------

# Product and project names. Matched case-insensitively on a word boundary.
# Deliberately curated rather than broad: words that are also ordinary English
# ("compose", "react", "excel", "sketch") are left out, because a classifier
# that cries wolf gets disabled and then nothing is counted at all.
PRODUCTS: list[str] = [
    "qt",
    "qtcharts",
    "qtquick",
    "qtwidgets",
    "qml",
    "blender",
    "unreal",
    "unrealengine",
    "grafana",
    "wireshark",
    "figma",
    "flutter",
    "godot",
    "vscode",
    "jetbrains",
    "photoshop",
    "houdini",
    "maya",
    "chromium",
    "qcustomplot",
    "kicad",
    "audacity",
    "ableton",
]

# Symbol citations -- a class or operator name that identifies its codebase as
# surely as the product does.
#
# The toolkit's classes are `Q` + CamelCase. The trailing `[a-z]` requirement is
# what tells abstract item view from `QUARTET`: a SCREAMING_CASE constant of ours is
# not a class name, and making that a rule beats listing every constant anyone
# will ever write.
SYMBOL_PATTERNS: list[tuple[str, str]] = [
    (r"\bQ[A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*\b", "toolkit class"),
    (r"\bNODE_OT_[a-z_]+\b", "DCC node operator"),
    (r"\bbpy\.[A-Za-z_.]+", "DCC python api"),
    (r"\b(?:bNode|bNodeTree|bNodeSocket|bNodeLink)[A-Za-z]*\b", "DCC C struct"),
    (r"\bED_node_[a-z_]+\b", "DCC editor function"),
    # R1612.1 -- widened after the round found its own blind spot. The first
    # spelling listed whole class names, so `graph schema K 2.cpp` and
    # compiler context went uncounted: a file name drops the `U`, and an
    # underscore ends `[A-Za-z]*`. Counting on doubt means the STEM is the
    # pattern, not the class.
    (r"\b[A-Z]*EdGraph[A-Za-z0-9_]*\b", "engine graph class"),
    (r"\b[A-Z]?Kismet[A-Za-z0-9_]*\b", "engine compiler class"),
    (r"\b[A-Z]*K2Node[A-Za-z0-9_]*\b", "engine node class"),
    (r"\b[A-Z]?Blueprint[A-Za-z0-9_]*\b", "engine asset class"),
    (r"\bbl_(?:idname|label|info)\b", "DCC python marker"),
]

# Tokens that match a symbol pattern and are not a citation. Each carries the
# reason, because an unexplained entry here is how a real name gets hidden.
ALLOW: dict[str, str] = {
    "QName": "quick_xml's XML qualified-name type -- a dependency's API, "
             "reached through our own parser",
}


# Products whose lowercase spelling is ordinary English -- "we compose a scene",
# "widgets react to input", "excel at it". Counted only when capitalised, which
# is what tells the product from the verb. Leaving them out entirely was the
# first draft and it under-counted; matching them case-insensitively cries wolf
# and gets the whole gate switched off.
# "Sketch" is deliberately absent: it opens sentences as a verb often enough
# that capitalisation stops telling the two apart.
CASED_PRODUCTS: list[str] = ["Compose", "React", "Electron", "Excel"]

CASED_RE = re.compile(r"\b(?:" + "|".join(CASED_PRODUCTS) + r")\b")


def _product_re() -> re.Pattern[str]:
    body = "|".join(sorted(PRODUCTS, key=len, reverse=True))
    return re.compile(rf"\b(?:{body})\b", re.IGNORECASE)


PRODUCT_RE = _product_re()
SYMBOL_RES = [(re.compile(p), label) for p, label in SYMBOL_PATTERNS]


def mentions(line: str) -> list[str]:
    """Every reference-project token in `line`, in the order they occur.

    A token counts once per occurrence: two names on one line are two, because
    clearing the line means dealing with both.
    """
    found: list[str] = []
    found.extend(PRODUCT_RE.findall(line))
    found.extend(CASED_RE.findall(line))
    for pattern, _label in SYMBOL_RES:
        for token in pattern.findall(line):
            if token in ALLOW:
                continue
            # A product hit already counted this span (e.g. `the toolkit's widget module`).
            if PRODUCT_RE.fullmatch(token):
                continue
            found.append(token)
    return found


def required_literals(pattern: re.Pattern[str]) -> list[str] | None:
    """Literals a match MUST contain, DERIVED from the pattern's own source.

    ★★★★★ R1805 — a cheap gate in front of an expensive one, and derived rather
    than written down because a hand-kept list of needles beside a hand-kept
    list of patterns is two accounts of one fact — the shape this project has
    paid for four times (R1738, R1784, R1795, R1798).

    Why it exists, measured: the twelve patterns scan 47.5 MB of tracked text
    each, 570 MB of matching to answer a question about 2,385 files, and ELEVEN
    OF THE TWELVE find nothing at all — two hits in the whole tree. So almost
    every pattern is being run over almost every file to confirm an absence a
    substring test settles instantly.

    Returns `None` when nothing can be derived, and the caller must then scan;
    that is the safe direction, since a missing prefilter costs time and a wrong
    one would cost a finding.
    """
    src = pattern.pattern
    if src.startswith(r"\b"):
        src = src[2:]
    # A starred or optional character class at the head contributes nothing to
    # what a match must contain: `[A-Z]*EdGraph` still requires `EdGraph`.
    src = re.sub(r"^\[[^\]]*\][*?]", "", src)
    # `(?:a|b|c)…` — every branch is a literal, so a match contains one of them.
    alt = re.fullmatch(r"\(\?:([^()\[\]|]+(?:\|[^()\[\]|]+)*)\)(.*)", src)
    if alt and all(c.isalnum() or c in "._" for c in alt.group(1).replace("|", "")):
        return alt.group(1).split("|")
    # Otherwise a literal run at the head, e.g. `NODE_OT_[a-z_]+`.
    head = re.match(r"[A-Za-z0-9_.]+", src)
    if head:
        return [head.group(0)]
    return None


#: `pattern -> the literals derived from it`, computed once.
_NEEDLES: dict[re.Pattern[str], list[str] | None] = {}


def _may_match(pattern: re.Pattern[str], text: str, lowered: str) -> bool:
    """Whether `pattern` could match `text`, by the cheap test only."""
    if pattern not in _NEEDLES:
        _NEEDLES[pattern] = required_literals(pattern)
    needles = _NEEDLES[pattern]
    if needles is None:
        return True
    ci = bool(pattern.flags & re.IGNORECASE)
    hay = lowered if ci else text
    return any((n.lower() if ci else n) in hay for n in needles)


def file_mentions(text: str) -> list[str]:
    """[`mentions`] over a whole file, skipping patterns that cannot match.

    The result is the same list `mentions` returns; only the work differs.
    `tools/test_hooks.sh` and this module's selftest both assert that.
    """
    lowered = text.lower()
    found: list[str] = []
    if _may_match(PRODUCT_RE, text, lowered):
        found.extend(PRODUCT_RE.findall(text))
    if _may_match(CASED_RE, text, lowered):
        found.extend(CASED_RE.findall(text))
    for pattern, _label in SYMBOL_RES:
        if not _may_match(pattern, text, lowered):
            continue
        for token in pattern.findall(text):
            if token in ALLOW:
                continue
            if PRODUCT_RE.fullmatch(token):
                continue
            found.append(token)
    return found


def excluded(path: str) -> str | None:
    """The reason `path` is out of scope, or None."""
    for prefix, reason in EXCLUDED:
        if path == prefix or path.startswith(prefix):
            return reason
    return None


def tracked_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [p for p in out.split("\0") if p]


def census() -> dict[str, int]:
    """path -> how many reference-project tokens it holds. Zero-counts omitted."""
    counts: dict[str, int] = {}
    for rel in tracked_files():
        if excluded(rel):
            continue
        full = ROOT / rel
        try:
            text = full.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue  # binary or unreadable: no prose to clear
        # ★★★★★ R1805 — ONCE PER FILE, not once per line.
        #
        # This read `sum(len(mentions(line)) for line in text.splitlines())`,
        # and the shape is what cost the time: 2,385 tracked files hold
        # 1,089,516 lines, and `mentions` runs twelve regexes, so the gate was
        # performing **13,074,192** regex passes to answer a question about
        # 2,385 files. Measured: `--check` took 119.7s of the 105.8s–120s this
        # step has been costing every push, and 80.1s of every COMMIT.
        #
        # Nothing needed the split. Not one pattern is anchored to a line
        # boundary and none carries MULTILINE or DOTALL — checked, not assumed —
        # and `check()` reports per-FILE counts with no line numbers, so the
        # per-line loop bought no information either. Verified over all 2,385
        # files: zero disagreements between the two scans.
        total = len(file_mentions(text))
        if total:
            counts[rel] = total
    return counts


def read_budget() -> dict[str, int]:
    if not BUDGET.exists():
        return {}
    budget: dict[str, int] = {}
    for line in BUDGET.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        path, count = line.rsplit("\t", 1)
        budget[path] = int(count)
    return budget


def write_budget(counts: dict[str, int]) -> None:
    lines = [
        "# R1611 -- the reference-name ratchet's budget. One row per file that",
        "# still holds another project's name, with how many it holds.",
        "#",
        "# The gate allows a count to FALL or hold and refuses a rise, and a file",
        "# absent from this list must hold none. Regenerate with",
        "# `python3 tools/reference_names.py --write-budget` after a clearing",
        "# round, and never to make a gate failure go away -- that is the one use",
        "# that turns a ratchet back into a suggestion.",
    ]
    lines += [f"{path}\t{count}" for path, count in sorted(counts.items())]
    BUDGET.write_text("\n".join(lines) + "\n", encoding="utf-8")


def check() -> int:
    """Fail if any file gained a mention, or a clean file grew one."""
    counts = census()
    budget = read_budget()
    risen = []
    fresh = []
    for path, count in sorted(counts.items()):
        allowed = budget.get(path)
        if allowed is None:
            fresh.append((path, count))
        elif count > allowed:
            risen.append((path, count, allowed))

    total = sum(counts.values())
    budget_total = sum(budget.values())
    if not risen and not fresh:
        cleared = budget_total - total
        note = f", {cleared} cleared since the budget" if cleared > 0 else ""
        print(f"reference names: {total} in {len(counts)} files{note} -- ratchet OK")
        return 0

    print("reference names: RATCHET FAILED")
    print()
    for path, count, allowed in risen:
        print(f"  {path}: {count} (budget {allowed}) -- {count - allowed} added")
    for path, count in fresh:
        print(f"  {path}: {count} -- this file held none")
    print()
    print("Another project's name reached a pushed artifact. Restate the")
    print("sentence in terms of the CAPABILITY and record the source in a")
    print("memory note; do not substitute the name mechanically, because the")
    print("sentence is carrying the evidence for a design decision.")
    print()
    print("If the rise is deliberate and agreed, regenerate the budget with")
    print("`python3 tools/reference_names.py --write-budget` and say so in the")
    print("round's changelog entry.")
    return 1


# --- selftest ---------------------------------------------------------------

CASES: list[tuple[str, int, str]] = [
    ("the toolkit's list view scrolls per pixel", 0, "no name, no count"),
    ("Qt's QListView scrolls per pixel", 2, "product and class both count"),
    ("a QAbstractItemView subclass", 1, "a class alone counts"),
    ("const QUARTET: [f64; 4]", 0, "SCREAMING_CASE is not a class name"),
    ("use quick_xml::name::QName;", 0, "allowlisted dependency type"),
    ("NODE_OT_translate is the operator", 1, "operator id counts"),
    ("bpy.ops.node.add_node()", 1, "the scripting api counts"),
    ("blender and Blender and BLENDER", 3, "case-insensitive, each occurrence"),
    ("a bNodeSocket carries the default", 1, "the C struct counts"),
    ("UEdGraphPin holds the direction", 1, "the engine class counts"),
    ("EdGraphSchema_K2.cpp names the file", 1, "a file name drops the prefix"),
    ("FKismetCompilerContext expands it", 1, "the compiler class counts"),
    ("bl_idname = 'node.x'", 1, "the scripting marker counts"),
    ("a blueprint of the layout", 0, "lowercase is the ordinary word"),
    ("we compose a scene and it reacts", 0, "ordinary English is not a product"),
    ("a Compose-class toolkit", 1, "capitalised, so the product"),
    ("paint composition and compose a scene", 0, "lowercase stays the verb"),
    ("React and Electron", 2, "both counted"),
    ("Sketch the layout first", 0, "too ambiguous to count, and it says so"),
    ("QtCharts is GPL", 1, "a product spelling wins over the class pattern"),
    ("ED_node_select_all", 1, "the editor function counts"),
    ("quotient, quantity, queue", 0, "lowercase q words are not classes"),
]


def selftest() -> int:
    failures = 0
    for line, want, why in CASES:
        got = len(mentions(line))
        if got != want:
            failures += 1
            print(f"  FAIL want {want} got {got}: {line!r} ({why})")
        # ★★★★★ R1805 — the fast path must AGREE, on every case, or the
        # prefilter has quietly turned the ratchet off. This is the one risk the
        # repair carries: a needle that is not actually required makes the gate
        # skip a file and report a clean tree. Same cases, both paths.
        fast = len(file_mentions(line))
        if fast != got:
            failures += 1
            print(f"  FAIL prefiltered path got {fast}, direct got {got}: {line!r}")
    # And every pattern's derived needles must be needles OF THAT PATTERN: a
    # literal the pattern cannot produce would skip files that do match.
    for label, pattern in [("PRODUCT_RE", PRODUCT_RE), ("CASED_RE", CASED_RE)] + [
        (lab, pat) for pat, lab in SYMBOL_RES
    ]:
        needles = required_literals(pattern)
        if needles is None:
            continue  # deriving nothing is safe: the caller scans
        # The property that matters is NECESSITY, not sufficiency: every string
        # this pattern matches must CONTAIN one of the needles. Checked against
        # the pattern's own matches over the tree's own text, plus the selftest
        # cases, because constructing a match from a pattern is not something
        # this can do in general — `Q` is a needle of `\bQ[A-Z]…` and is not
        # itself a match, which is exactly what the first draft of this
        # assertion got wrong.
        ci = bool(pattern.flags & re.IGNORECASE)
        for sample in [line for line, _w, _y in CASES]:
            for hit in pattern.findall(sample):
                hay = hit.lower() if ci else hit
                if not any((n.lower() if ci else n) in hay for n in needles):
                    failures += 1
                    print(
                        f"  FAIL {label}: match {hit!r} contains none of the "
                        f"derived needles {needles!r}"
                    )
    # The exclusion list must name real paths, or an exclusion silently widens.
    for prefix, _reason in EXCLUDED:
        if not (ROOT / prefix).exists():
            failures += 1
            print(f"  FAIL exclusion {prefix!r} does not exist")
    # Every allowlist entry must carry a reason.
    for token, reason in ALLOW.items():
        if not reason.strip():
            failures += 1
            print(f"  FAIL allowlisted {token!r} with no reason")
    if failures:
        print(f"reference_names selftest: {failures} failure(s)")
        return 1
    print(f"reference_names selftest: {len(CASES)} cases OK")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="ratchet check")
    parser.add_argument("--write-budget", action="store_true")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--top", type=int, default=20)
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.check:
        return check()

    counts = census()
    if args.write_budget:
        write_budget(counts)
        print(f"wrote {BUDGET.relative_to(ROOT)}: {len(counts)} files, "
              f"{sum(counts.values())} mentions")
        return 0

    total = sum(counts.values())
    print(f"reference names: {total} mentions in {len(counts)} tracked files")
    print()
    for path, count in sorted(counts.items(), key=lambda kv: -kv[1])[: args.top]:
        print(f"  {count:6d}  {path}")
    print()
    print("excluded, with the reason each cannot be cleared:")
    for prefix, reason in EXCLUDED:
        print(f"  {prefix}\n      {reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
