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
        "tools/reference_names.py",
        "this file IS the term list; it cannot avoid holding the terms.",
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
# what tells `QAbstractItemView` from `QUARTET`: a SCREAMING_CASE constant of
# ours is not a class name, and making that a rule beats listing every constant
# anyone will ever write.
SYMBOL_PATTERNS: list[tuple[str, str]] = [
    (r"\bQ[A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*\b", "toolkit class"),
    (r"\bNODE_OT_[a-z_]+\b", "DCC node operator"),
    (r"\bbpy\.[A-Za-z_.]+", "DCC python api"),
    (r"\b(?:bNode|bNodeTree|bNodeSocket|bNodeLink)[A-Za-z]*\b", "DCC C struct"),
    (r"\bED_node_[a-z_]+\b", "DCC editor function"),
    (r"\b(?:UEdGraph|UK2Node|FEdGraph|SGraphNode|UBlueprint|FBlueprint|EEdGraphPin)"
     r"[A-Za-z]*\b", "engine graph class"),
]

# Tokens that match a symbol pattern and are not a citation. Each carries the
# reason, because an unexplained entry here is how a real name gets hidden.
ALLOW: dict[str, str] = {
    "QName": "quick_xml's XML qualified-name type -- a dependency's API, "
             "reached through our own parser",
}


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
    for pattern, _label in SYMBOL_RES:
        for token in pattern.findall(line):
            if token in ALLOW:
                continue
            # A product hit already counted this span (e.g. `QtWidgets`).
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
        total = sum(len(mentions(line)) for line in text.splitlines())
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
    ("we compose a scene and it reacts", 0, "ordinary English is not a product"),
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
