#!/usr/bin/env python3
"""R1945.2 — a doc link into feature-gated code, and the label that carries it.

WHAT THIS EXISTS FOR, measured rather than imagined. `cargo doc -p
pinion-runtime --no-deps --document-private-items` — the crate's OWN default
configuration — answered **seven** `unresolved link` errors on 2026-09-01, and
had for as long as those links had been written. Ungated prose in `lib.rs`,
`layout.rs` and `paint_cache_stats.rs` linked into `paint_adapter`,
`text_engine` and `image_cache`, all three of which exist only under the
`vello` feature.

Nothing saw it, and the two reasons are worth keeping:

  * CI documents the WORKSPACE with `--features pinion-runtime/vello`, where
    every one of them resolves;
  * the per-crate `pre-push` gate (R1916) documents only the crates a push
    touches, and `crates/pinion-runtime/` had not been touched since R1905.

★ THE REPAIR, AND WHY IT NEEDS THIS FILE. The prose is kept in ONE copy: the
text carries a markdown REFERENCE link with a kebab-case label —

    /// … the §5.37 paint arm ([`to_vello_with_text_engine`][vello-paint-with-engine])
    #[cfg_attr(feature = "vello", doc = "[vello-paint-with-engine]: crate::paint_adapter::to_vello_with_text_engine")]

— and the label's DEFINITION is supplied only when the item it names exists.
With the feature, rustdoc resolves the definition and the reader gets a
hyperlink; without it the label is undefined, pulldown-cmark renders the text
verbatim, and nothing is asked to resolve. The alternative — a `cfg_attr` pair
carrying the sentence twice — would be one rule with two spellings, which is
the exact defect R1945 repaired in `paint_adapter.rs`.

★★★★★ AND THE HAZARD THAT MAKES THIS A GATE RATHER THAN A NOTE: the label is
kebab-case precisely so rustdoc does NOT read it as a Rust path — which means a
use whose definition is missing or misspelled is **silent in both
configurations**. It renders as literal `[`x`][some-label]` text and no lint
fires. A convention with a silent failure mode and no gate is what this
repository keeps paying for, so the convention arrives with its check.

WHAT IT CHECKS, and the limit stated rather than hidden: within one FILE, the
set of kebab-label USES and the set of kebab-label DEFINITIONS must be equal. A
use with no definition is the silent failure above; a definition with no use is
dead weight that will be read as live. It does NOT check that the definition
sits in the same doc BLOCK as its use — a markdown reference definition is
scoped to one doc string, so a definition on the wrong item is a defect this
file cannot see. What it also does not need to check is whether the target path
resolves: rustdoc does that itself, in the configuration where the definition
exists, and both homes of that gate (CI's featured workspace run and
`pre-push`'s per-crate one) still run.

Usage:
    python3 tools/feature_gated_doc_links.py --check     # the gate (pre-push)
    python3 tools/feature_gated_doc_links.py --selftest  # its own tests
    python3 tools/feature_gated_doc_links.py --list      # every label, with its file
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Where Rust source lives here. `crates/` is the framework and `examples/` the
#: demos; both carry doc comments and both are documented by CI.
SOURCE_ROOTS = ("crates", "examples")

#: A kebab-case markdown reference label — at least one hyphen, so it can never
#: be read as a Rust path. That is the whole point: rustdoc leaves it alone.
LABEL = r"[a-z0-9]+(?:-[a-z0-9]+)+"

#: A USE: `[anything][kebab-label]`. The text half is non-greedy and may not
#: contain a `]`, so `[a][b][c]` reads as one use of `c` rather than of `b`.
USE = re.compile(rf"\][\[]({LABEL})\]".replace("[[", "["))

#: A DEFINITION: a line whose content is `[kebab-label]: <target>`. Leading
#: `///` / `//!` / a `doc = "` prefix are stripped before matching, so both
#: spellings of a doc line are read the same way.
DEFINITION = re.compile(rf"^\[({LABEL})\]:\s*\S")

#: The prefixes that make a source line documentation rather than code.
DOC_PREFIXES = ("///", "//!")


def doc_text(line: str) -> str | None:
    """The markdown a source line contributes, or `None` if it contributes none.

    Two spellings reach rustdoc as the same thing and must reach this census as
    the same thing too: a `///` / `//!` comment, and a `doc = "…"` string inside
    an attribute (which is how a conditionally-supplied definition has to be
    written, since `cfg_attr` takes attributes and not comments).
    """
    stripped = line.strip()
    for prefix in DOC_PREFIXES:
        if stripped.startswith(prefix):
            return stripped[len(prefix) :].strip()
    match = re.search(r'\bdoc\s*=\s*"(.*)"', stripped)
    if match:
        # `\[` inside a Rust string is not an escape, but `\"` is; nothing here
        # needs a quote, so the raw span is the markdown.
        return match.group(1).strip()
    return None


def scan(source: str) -> tuple[set[str], set[str]]:
    """The kebab labels a file USES and the ones it DEFINES."""
    uses: set[str] = set()
    defined: set[str] = set()
    for line in source.splitlines():
        text = doc_text(line)
        if text is None:
            continue
        found = DEFINITION.match(text)
        if found:
            defined.add(found.group(1))
            continue
        uses.update(USE.findall(text))
    return uses, defined


def sources() -> list[pathlib.Path]:
    out: list[pathlib.Path] = []
    for root in SOURCE_ROOTS:
        out.extend(sorted((ROOT / root).rglob("*.rs")))
    return out


def survey() -> list[tuple[pathlib.Path, set[str], set[str]]]:
    """Every file that uses or defines at least one kebab label."""
    out = []
    for path in sources():
        uses, defined = scan(path.read_text(encoding="utf-8", errors="replace"))
        if uses or defined:
            out.append((path, uses, defined))
    return out


def check() -> int:
    problems: list[str] = []
    files = 0
    labels = 0
    for path, uses, defined in survey():
        files += 1
        labels += len(uses | defined)
        rel = path.relative_to(ROOT)
        for label in sorted(uses - defined):
            problems.append(
                f"{rel}: `[{label}]` is used and never defined — the text will "
                f"render verbatim in EVERY configuration, and no lint says so"
            )
        for label in sorted(defined - uses):
            problems.append(
                f"{rel}: `[{label}]` is defined and never used — a dead label "
                f"reads as a live one"
            )
    for line in problems:
        print(f"gated doc links: {line}", file=sys.stderr)
    if problems:
        return 1
    print(f"gated doc links: {labels} label(s) in {files} file(s) — paired OK")
    return 0


def selftest() -> int:
    cases: list[tuple[str, str, set[str], set[str]]] = [
        (
            "a use in a doc comment is a use",
            '/// see [`x`][vello-thing] here\n',
            {"vello-thing"},
            set(),
        ),
        (
            "a definition in a cfg_attr doc string is a definition",
            '#[cfg_attr(feature = "vello", doc = "[vello-thing]: crate::x")]\n',
            set(),
            {"vello-thing"},
        ),
        (
            "an inner doc line carries both forms",
            '//! [`x`][a-b]\n#![cfg_attr(feature = "v", doc = "[a-b]: crate::x")]\n',
            {"a-b"},
            {"a-b"},
        ),
        # ★ A Rust path is NOT a kebab label, and this is the case the whole
        # design turns on: an ordinary intra-doc link must not be counted here,
        # because rustdoc already owns it.
        (
            "an ordinary intra-doc link is not a label",
            "/// see [`crate::paint_adapter::to_vello`]\n",
            set(),
            set(),
        ),
        (
            "an explicit path link is not a label either",
            "/// see [`to_vello`](crate::paint_adapter::to_vello)\n",
            set(),
            set(),
        ),
        # ★★ CODE IS NOT DOCUMENTATION. A slice index or an array literal in
        # real code can look like a reference link; only doc lines are read.
        (
            "code outside a doc comment is not read",
            'let s = m["a"]["some-key"];\n',
            set(),
            set(),
        ),
        (
            "a plain comment is not documentation",
            "// [`x`][some-label] in a note\n",
            set(),
            set(),
        ),
        (
            "a single-word label is not kebab and is left alone",
            "/// see [`x`][thing]\n",
            set(),
            set(),
        ),
        (
            "two uses on one line are both counted",
            "/// [`a`][one-a] and [`b`][two-b]\n",
            {"one-a", "two-b"},
            set(),
        ),
        (
            "a definition with no target is not a definition",
            '/// [some-label]:\n',
            set(),
            set(),
        ),
    ]
    failed = 0
    for name, source, want_uses, want_defs in cases:
        uses, defined = scan(source)
        if uses != want_uses or defined != want_defs:
            failed += 1
            print(
                f"FAIL: {name}: want uses={sorted(want_uses)} "
                f"defs={sorted(want_defs)}, got uses={sorted(uses)} "
                f"defs={sorted(defined)}",
                file=sys.stderr,
            )
    # ★★★★★ THE POPULATION MUST NOT BE EMPTY. A pairing check over nothing
    # passes, and would go on passing after the convention it guards was
    # deleted — the class this repository has been caught by repeatedly. The
    # convention has live users; assert that it does.
    live = survey()
    if not live:
        failed += 1
        print(
            "FAIL: no file in the tree uses this convention, so --check "
            "asserts nothing — either it was removed (delete this gate too) "
            "or the census stopped seeing it",
            file=sys.stderr,
        )
    print(f"gated doc links selftest: {len(cases) - failed} of {len(cases)} cases OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="the gate")
    parser.add_argument("--selftest", action="store_true", help="its own tests")
    parser.add_argument("--list", action="store_true", help="every label, by file")
    args = parser.parse_args()

    if args.selftest:
        return selftest()
    if args.list:
        for path, uses, defined in survey():
            rel = path.relative_to(ROOT)
            for label in sorted(uses | defined):
                marks = "".join(
                    (
                        "u" if label in uses else "-",
                        "d" if label in defined else "-",
                    )
                )
                print(f"{marks}  {rel}  {label}")
        return 0
    return check()


if __name__ == "__main__":
    sys.exit(main())
