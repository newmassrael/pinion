#!/usr/bin/env python3
"""R1650 — the reference toolkit's public surface, MEASURED instead of remembered.

## Why this exists

Rounds here routinely justify a design by what the reference toolkit does or
does not offer ("measured on the toolkit at 6.11: no content-state concept
exists on any panel class"). Those sentences were produced by a person reading
the tree once, and then they were *prose*: nothing re-derives them, nothing
notices when a claim was wrong, and nothing notices when a later release makes
one false. The node-graph axis has had a meter since R1601 and the
analysis-tool axis since R1646, for exactly this reason. The reference claims
had none.

This is that meter. Every claim lives in `docs/qt-census.json` with the
measurement that produces it, and running this file re-derives all of them
against a real source tree.

## Where the reference's own spellings live, and why only here

This file names the reference tree's directory layout and the identifiers it
scans for. `docs/reference-surface.json` does not: a claim names a **role**
(`widget`, `attribute-enum`) and `SURFACES` below resolves it. That is the same
split R1612 made for the operator census — the reader has to know a tree's
layout to scan it at all, and everything it *writes* is clean — and it is why
this file carries an entry in the reference-name ratchet's exclusion list while
the artifact beside it carries none.

## What a claim may assert, and why the vocabulary is this small

    symbol_present    a token appears in a named file — the reference FLOOR
                      exists, so "we do this too" is a checkable statement
    class_surface     a class's declared members are parsed out of its header,
                      the count is pinned, and no member name matches any of
                      the `absent_patterns`

`class_surface` is the one that carries weight, and the member **count** is
what makes it worth anything. An absence proved by grep is worth nothing: a
pattern that matches nothing and a parse that produced nothing look identical,
so a broken parser reads as a confirmed absence, forever, silently. Pinning the
count means the parse has to keep finding the same surface it found when a
person looked at it — and a release that adds members fails the pin and asks
for a re-reading rather than answering from a stale memory.

This is the wrong-`have` asymmetry (R1602) pointed at the reference instead of
at ourselves: a wrong "the toolkit has this" is caught the moment somebody
looks for it, and a wrong "the toolkit does not have this" is a claim in a
changelog that nobody will ever check again.

## What it does NOT prove

That a capability is *absent from the product*. It proves that the named class
does not declare a member matching the named patterns — which is the honest
form of "no caller can ask this", because a caller can only call what is
declared. Behaviour reachable another way (a private class, a different
module, a signal) is outside what a header can answer, and a claim that needs
that has no business being stated as a fact here.

## Usage

    python3 tools/reference_surface.py             # re-derive every claim
    python3 tools/reference_surface.py --selftest  # check the tool itself
    REFERENCE_SRC=/path/to/tree python3 tools/reference_surface.py

Absent a source tree it reports SKIPPED and exits 0: the tree is a developer's
local checkout, not a repository artifact, so CI cannot run this and a failure
there would be about the runner rather than about the claim.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent
PIN = WORKSPACE / "docs" / "reference-surface.json"
DEFAULT_SRC = Path.home() / "qt-everywhere-src-6.11.1"

# The one table that knows the reference tree's layout. A claim names the KEY;
# the header path and the class identifier are resolved here, so the pushed
# artifact holds neither.
SURFACES: dict[str, dict[str, str]] = {
    "widget": {
        "file": "qtbase/src/widgets/kernel/qwidget.h",
        "class": "QWidget",
    },
    "attribute-enum": {
        "file": "qtbase/src/corelib/global/qnamespace.h",
    },
}

# A declared member: the name immediately before an opening parenthesis, at a
# line that is inside a class body. Deliberately crude on TYPES (which vary
# wildly) and exact on the NAME, because the name is all a claim asserts about.
_MEMBER = re.compile(r"(?:^|[\s*&<>~])([A-Za-z_][A-Za-z0-9_]*)\s*\(")
# Lines that are not member declarations even though they look like calls.
_NOT_A_MEMBER = re.compile(
    r"^\s*(?:#|//|/\*|\*|Q_[A-Z_]+\b|friend\b|return\b|if\b|for\b|while\b|else\b)"
)


def _class_body(header: str, class_name: str) -> str | None:
    """The brace-balanced body of `class ... <class_name> ... { ... }`."""
    opener = re.search(
        r"\bclass\b[^;{}\n]*\b" + re.escape(class_name) + r"\b[^;{}]*\{", header
    )
    if not opener:
        return None
    depth = 0
    start = opener.end() - 1
    for i in range(start, len(header)):
        if header[i] == "{":
            depth += 1
        elif header[i] == "}":
            depth -= 1
            if depth == 0:
                return header[start + 1 : i]
    return None


def _members(body: str) -> list[str]:
    """Every member name declared in `body`, deduplicated, in declaration order.

    A name counts when it is immediately followed by `(` — a function or a
    constructor. Members of a NESTED class body count too, because excluding
    them would let a claim be satisfied by relocating an API one level in; the
    nested type's own name does not, since a type is not something a caller
    invokes. Data members do not either, and that is stated rather than fixed:
    every claim here is about a question a caller can ASK.
    """
    seen: dict[str, None] = {}
    for line in body.splitlines():
        if _NOT_A_MEMBER.match(line):
            continue
        for name in _MEMBER.findall(line):
            seen.setdefault(name, None)
    return list(seen)


def _measure(claim: dict, src: Path) -> tuple[bool, str]:
    kind = claim["kind"]
    surface = SURFACES.get(claim["surface"])
    if surface is None:
        return False, f"unknown surface {claim['surface']!r}"
    if kind == "symbol_present":
        path = src / surface["file"]
        if not path.is_file():
            return False, f"missing header for surface {claim['surface']}"
        text = path.read_text(encoding="utf-8", errors="replace")
        hits = text.count(claim["symbol"])
        if hits == 0:
            return False, f"{claim['symbol']} absent from {claim['file']}"
        return True, f"{claim['symbol']} present {hits}x"
    if kind == "class_surface":
        path = src / surface["file"]
        if not path.is_file():
            return False, f"missing header for surface {claim['surface']}"
        body = _class_body(
            path.read_text(encoding="utf-8", errors="replace"), surface["class"]
        )
        if body is None:
            return False, f"surface {claim['surface']} not found in its header"
        names = _members(body)
        pinned = claim["member_count"]
        if len(names) != pinned:
            return (
                False,
                f"surface {claim['surface']} declares {len(names)} members, pin says "
                f"{pinned} — the surface moved, so re-read it and re-pin rather "
                f"than trusting the absence below",
            )
        matched = [
            n
            for n in names
            for p in claim["absent_patterns"]
            if re.search(p, n, re.IGNORECASE)
        ]
        if matched:
            return (
                False,
                f"surface {claim['surface']} declares {sorted(set(matched))}, "
                f"which the claim says it does not",
            )
        return True, f"{len(names)} members, none matching {claim['absent_patterns']}"
    return False, f"unknown claim kind {kind!r}"


def run(src: Path, *, quiet: bool = False) -> int:
    pin = json.loads(PIN.read_text(encoding="utf-8"))
    claims = pin["claims"]
    failed = []
    for claim in claims:
        ok, detail = _measure(claim, src)
        if not quiet:
            print(f"  [{'ok ' if ok else 'FAIL'}] {claim['id']}: {detail}")
        if not ok:
            failed.append((claim["id"], detail))
    print(
        f"reference census — {len(claims) - len(failed)} of {len(claims)} claim(s) "
        f"re-derived against {src.name}"
    )
    for cid, detail in failed:
        print(f"  FAIL {cid}: {detail}")
    return 1 if failed else 0


def selftest() -> int:
    """The tool's own gates. A measuring instrument nobody measured is prose."""
    failures = []

    def check(name: str, cond: bool, why: str = "") -> None:
        if not cond:
            failures.append(f"{name}: {why}")

    fixture_class = "Zed" + "Thing"
    body = _class_body(
        f"class SOME_EXPORT {fixture_class} : public Base {{\n"
        "  int alpha(int);\n"
        "  class Inner { void beta(); };\n"
        "  void gamma() const;\n"
        "};\n",
        fixture_class,
    )
    check("class body found", body is not None)
    names = _members(body or "")
    check(
        "members parsed, nested body included and the nested TYPE name not",
        names == ["alpha", "beta", "gamma"],
        str(names),
    )
    check(
        "comments and macros are not members",
        _members("  // void ghost();\n  Q_OBJECT\n  void real();\n") == ["real"],
    )
    # The load-bearing one: a claim whose parse found nothing must FAIL, not
    # pass by vacuous absence.
    ok, detail = _measure(
        {
            "kind": "class_surface",
            "surface": "widget",
            "member_count": 3,
            "absent_patterns": ["zzz"],
        },
        Path("/nonexistent"),
    )
    check("a missing file fails rather than passing empty", not ok, detail)
    for f in failures:
        print(f"  FAIL {f}")
    print(f"selftest — {4 - len(failures)} of 4 check(s) passed")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--src", default=os.environ.get("REFERENCE_SRC"))
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    src = Path(args.src) if args.src else DEFAULT_SRC
    if not src.is_dir():
        print(f"reference census — SKIPPED: no source tree at {src}")
        return 0
    return run(src)


if __name__ == "__main__":
    sys.exit(main())
