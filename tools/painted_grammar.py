#!/usr/bin/env python3
"""R2163 §5.2 — the ONE reader of a crate's emitted address grammar.

## The round that paid for this

R2146 gave `pinion-chart` a committed artifact — `src/painted_grammar.tsv`,
every address template the crate paints, emitted from the declaration itself —
so a reader outside Rust can FORMAT an address instead of spelling one. That
artifact then grew two readers that never met:

  * `tools/rpc_verify.py` reads it so a walk can compose (`chart_address`,
    `chart_family`, `chart_overlay_address`).
  * `tools/painted_addresses.py` reads it so the census can say a family's
    value is pinned by an artifact rather than by an assertion.

Both need the same rule — **where does a template's FIXED run end** — and both
wrote their own copy of it. R2154 measured the copy in `chart_family` to be
wrong (`{prefix}.a11y.r{index}` cut at the first segment *starting* with a
brace, so the stem came back as `chart.a11y.r{index}`, braces and all: a string
naming nothing, which is this campaign's own failure mode) and fixed it THERE.
The census's copy kept the broken form for eight rounds.

⇒ ★★★★★ **a rule written twice is repaired once.** The repair belongs to the
rule, not to the caller that surfaced it, so the rule gets one home and both
callers ask it.

## The second divergence, which the first one hid

The census's copy was not only mis-cutting; it was answering a different
QUESTION in the same words. `grammar_parts` returned whole fixed RUNS
(`axis.x`, `grid.minor.x`, `colorbar.strip`) and `grammar_pins` compared them
against a family STEM, which the census derives as an address's first two
segments (`chart.axis`, `chart.grid`). Those two agree only when a run happens
to be one segment long — so every grammar with two fixed segments, and every
overlay template (whose first segment is `{part}`), was invisible to the pin it
genuinely provides.

Measured at R2163, before this module existed: **ten families carrying 98 walk
and 55 Rust sites** read as pinned by an assertion, or by NOTHING, while the
committed grammar pins them — `chart.inspect` (68 sites) the largest in the
whole queue, and `scatter.inspect` reported BLOCKED, which is the census's word
for *work no round may finish*.

## What this module owns

The artifact's format, and nothing else: where to find one, how to read its
rows, where a fixed run ends, and which family stems it therefore pins. What it
deliberately does NOT own is the *prefix* half of a pin — see
`painted_addresses.grammar_prefixes`, which reads the prefixes charts are
actually given from the Rust source. A part word like `bar` or `label` is
ordinary; claiming a family pinned when it is not loses a check silently, so
the unsafe half of that judgement stays anchored on both sides.
"""

from __future__ import annotations

import functools
import string
import sys
from pathlib import Path
from typing import Iterator

ROOT = Path(__file__).resolve().parent.parent

#: Where an emitted grammar lives. Both roots, deliberately: `pinion-chart` is
#: the only crate that emits one today, and the reason the mechanism has stayed
#: single is that nothing else could join. An example that emits one is a
#: participant the day it is written, with no list here to remember to edit.
ARTIFACTS = ("crates/*/src/painted_grammar.tsv", "examples/*/src/painted_grammar.tsv")

#: The kinds a row can carry, and what each one means.
#:
#: `const`   a whole address, already rendered
#: `grammar` a template taking `prefix` and the composer's own arguments
#: `overlay` a template taking `part` as well
#: `part`    one name `part` can be
KINDS = ("const", "grammar", "overlay", "part")


@functools.lru_cache(maxsize=1)
def artifacts() -> tuple[Path, ...]:
    """Every committed grammar artifact in this tree, in a stable order."""
    found: list[Path] = []
    for pattern in ARTIFACTS:
        found.extend(sorted(ROOT.glob(pattern)))
    return tuple(found)


def rows(path: Path) -> Iterator[tuple[str, str, str]]:
    """One `(kind, name, value)` per data row of `path`.

    Comments, blank lines and anything without two tabs are skipped — the same
    tolerance the artifact's own header describes, so a reader here and a reader
    in Rust see the same rows.
    """
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.count("\t") < 2:
            continue
        kind, name, value = line.split("\t", 2)
        yield kind, name, value


def table(path: Path) -> dict[str, dict[str, str]]:
    """One artifact as `kind -> name -> value`."""
    out: dict[str, dict[str, str]] = {}
    for kind, name, value in rows(path):
        out.setdefault(kind, {})[name] = value
    return out


def fields(segment: str) -> set[str]:
    """The placeholder names one dotted segment carries."""
    return {
        field
        for _text, field, _spec, _conv in string.Formatter().parse(segment)
        if field
    }


def carries_placeholder(segment: str) -> bool:
    """Whether `segment` has a placeholder ANYWHERE in it.

    ⚠⚠ The whole rule, and the one R2154 paid for. `r{index}` does not BEGIN
    with a placeholder and it is not a fixed segment: a cut that tests
    `startswith("{")` keeps it, and hands back a stem with braces in it — an
    address naming nothing, read by a walk as *the chart did not paint this*.
    """
    return bool(fields(segment))


def body(template: str) -> tuple[str, ...] | None:
    """`template`'s segments after its leading `{prefix}.`, or `None`.

    A template that does not open with its prefix is not one this reader can
    cut, and saying so is better than cutting it somewhere plausible.
    """
    head, marker, rest = template.partition("{prefix}")
    if head or not marker or not rest.startswith("."):
        return None
    return tuple(rest[1:].split("."))


def fixed_run(template: str) -> tuple[str, ...]:
    """The run of FIXED segments between `template`'s prefix and its first
    argument — `()` when it opens with one.

        {prefix}.area.{index}          -> ('area',)
        {prefix}.grid.minor.x.{index}  -> ('grid', 'minor', 'x')
        {prefix}.a11y.r{index}         -> ('a11y',)
        {prefix}.{part}.header         -> ()
    """
    segments = body(template)
    if segments is None:
        return ()
    fixed: list[str] = []
    for segment in segments:
        if carries_placeholder(segment):
            break
        fixed.append(segment)
    return tuple(fixed)


@functools.lru_cache(maxsize=1)
def family_heads() -> frozenset[str]:
    """Every word a committed grammar can put DIRECTLY after a prefix.

    That word is the second half of a family stem, which is the unit the census
    classifies an observed address by — so this is the set that answers "does an
    artifact pin this family", once a caller has also checked the prefix.

    Three cases, and the third is the one that keeps this safe:

    * a `grammar` row whose first segment is fixed contributes that word;
    * an `overlay` row opens with `{part}`, whose values the SAME artifact
      declares as `part` rows — a closed, published set, so each one
      contributes;
    * any other leading placeholder (`{prefix}.{part}.{index}` as a `grammar`
      row, the general indexed form) is a word the CALLER chooses. It could be
      anything, so it contributes NOTHING. Folding it in would claim every
      family in the tree as pinned, which is the direction that loses a check
      silently.
    """
    heads: set[str] = set()
    for path in artifacts():
        rows_by_kind = table(path)
        parts = tuple(rows_by_kind.get("part", {}).values())
        for kind in ("grammar", "overlay"):
            for template in rows_by_kind.get(kind, {}).values():
                segments = body(template)
                if not segments:
                    continue
                first = segments[0]
                if not carries_placeholder(first):
                    heads.add(first)
                elif kind == "overlay" and first == "{part}":
                    heads.update(parts)
    return frozenset(heads)


def selftest() -> int:
    """Prove the rules above on the cases that were measured wrong."""
    failed = 0

    def check(label: str, got: object, want: object) -> None:
        nonlocal failed
        if got != want:
            failed += 1
            print(f"FAIL: {label}: got {got!r}, wanted {want!r}", file=sys.stderr)

    # ── the cut ──────────────────────────────────────────────────────────────
    check("a lone fixed segment", fixed_run("{prefix}.area.{index}"), ("area",))
    check(
        "★★★★★ R2154's case: a placeholder INSIDE a segment ends the run",
        fixed_run("{prefix}.a11y.r{index}"),
        ("a11y",),
    )
    check(
        "a deep fixed run comes back whole",
        fixed_run("{prefix}.grid.minor.x.{index}"),
        ("grid", "minor", "x"),
    )
    check("a leading placeholder fixes nothing", fixed_run("{prefix}.{part}.header"), ())
    check("a template with no prefix is not cut", fixed_run("lab.node.{name}"), ())
    check("an address with no argument is all fixed", fixed_run("{prefix}.axis.x"), ("axis", "x"))
    check("a segment is placeholder-carrying by its braces", carries_placeholder("r{index}"), True)
    check("and a fixed one is not", carries_placeholder("a11y"), False)

    # ── the heads ────────────────────────────────────────────────────────────
    heads = family_heads()
    if not heads:
        failed += 1
        print(
            "FAIL: no grammar artifact was read — every head below is vacuous",
            file=sys.stderr,
        )
    for word, why in (
        ("area", "a one-segment grammar"),
        ("axis", "★ a DEEP grammar's first word — invisible until R2163"),
        ("grid", "★ the same, under two templates"),
        ("label", "★ the same"),
        ("colorbar", "★ the same"),
        ("a11y", "★ and the one R2154's cut used to mangle"),
        ("inspect", "★★ an overlay part, declared by the artifact itself"),
        ("playhead", "★★ the other one — this is why the set is not a guess"),
    ):
        if word not in heads:
            failed += 1
            print(f"FAIL: {why}: {word!r} is not a family head", file=sys.stderr)
    for word, why in (
        ("{part}", "a placeholder is never a head"),
        ("r{index}", "nor a mangled segment"),
        ("nosuchword", "nor a word no artifact carries"),
    ):
        if word in heads:
            failed += 1
            print(f"FAIL: {why}: {word!r} came back as a family head", file=sys.stderr)

    # ── and the artifact set is not empty, or every check above is vacuous ───
    if not artifacts():
        failed += 1
        print(
            "FAIL: no artifact matched "
            f"{list(ARTIFACTS)} — this reader is answering about nothing",
            file=sys.stderr,
        )
    for path in artifacts():
        missing = sorted(set(KINDS) - set(table(path)))
        if missing:
            failed += 1
            print(f"FAIL: {path} carries no {missing} row", file=sys.stderr)

    print(f"painted_grammar selftest: {'FAILED' if failed else 'OK'} ({failed} failure(s))")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(selftest())
