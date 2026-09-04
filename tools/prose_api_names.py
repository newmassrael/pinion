#!/usr/bin/env python3
"""★★★★★ R2000 — a name this tree's prose CITES must be a name this tree HAS.

Why this gate exists
--------------------

R2000 found three citations in one round's own new prose that named nothing:
``Document::may_reverse``, ``Document::reverse_targets`` and ``Document::reverse``
were the verb's names in a draft, the verb shipped as ``turn`` / ``may_turn`` /
``retarget``, and the prose kept the draft's vocabulary. A fourth,
``Document::reroute_flow``, had been in ``model.rs`` since R1934 — so this is not
a one-round slip, it is a class nothing was looking for.

★ **What makes the class invisible is that the two readers each have a blind
spot, and the citations land in the gap.**

* ``rustdoc`` resolves ``[`Document::turn`]``, an *intra-doc link* — and only
  inside a doc comment. A plain ``//`` comment is not documentation, so rustdoc
  does not read it at all, and one of R2000's three was exactly there.
* A backticked ``` `Document::turn` ``` is code *formatting*, not a link.
  Nothing resolves it, in a doc comment or out of one.

So the repair cannot be "make them all intra-doc links": half the population is
in comments rustdoc will never open. And rustdoc runs in CI only here (the push
hook does not carry it), which is its own registered debt — a broken link is
found after publishing. This gate runs at the commit and the push, over BOTH
kinds of comment.

What it checks
--------------

For every ``Type::method`` written inside a Rust comment, in backticks or
brackets: if this workspace DECLARES ``Type``, then some crate declaring it must
also declare a ``fn method``. A citation of a type we do not own (``Vec::push``,
``Result::map_err``) is out of the denominator — this tree is not the authority
on those names.

⚠ **The population is stated rather than assumed**, and ``--census`` prints it:
the count of citations examined, how many were in scope, and how many crates
contributed names. A gate whose denominator can silently go to zero is one that
passes vacuously, which is why ``--selftest`` asserts the denominator is large.

What it deliberately does NOT check
-----------------------------------

* ``Self::method``. Resolving it needs the enclosing ``impl``, which is a parser
  this does not have — and rustdoc already resolves the bracketed form of it.
  Naming the omission here rather than letting a reader infer completeness.
* Whether the method is *public*, or whether the type is the one the reader
  meant. This answers "does this name exist", which is the failure that
  happened. A citation naming a real member of the wrong type is a different
  defect and this gate does not claim it.

⚠ **A FIELD is a name too**, and finding that out was this gate's own first
measurement. The draft above this line said fields were out of the population
and the code counted them as refusals anyway — 626 of them, against 4 real ones
— which is precisely the class the gate was built for, reproduced inside it. A
reader following ``AccessState::disabled`` arrives somewhere; the gate's question
is whether they arrive, not what kind of member greets them. So the population
of names is ``fn`` **and** struct fields, and it is deliberately generous: an
over-wide population can only ever let a bad citation through, while an
over-narrow one refuses good prose, and a gate that cries wolf is one that gets
switched off.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# What stands today. Tracked, so the list is reviewable and its shrinking is a
# diff rather than a claim.
PIN = ROOT / "docs" / "prose-api-names.json"

# Where this tree's own Rust lives. `vendor/` is other repositories' and
# `target/` is build output; neither is prose this round can repair.
TREES = ("crates", "examples")

# A citation: `Type::method` or [`Type::method`], the two spellings this tree
# actually uses. Deliberately anchored on the backtick, because an unquoted
# `Type::method` in prose is usually being talked ABOUT rather than cited.
CITED = re.compile(r"`([A-Z][A-Za-z0-9]*)::([a-z_][a-z_0-9]*)`")

# A declaration of a type this workspace owns.
DECLARED = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union)\s+([A-Z][A-Za-z0-9]*)")

# A function this workspace defines. Any `fn`, at any visibility: the question
# is whether the NAME exists, not whether a caller outside could reach it.
DEFINED = re.compile(r"\bfn\s+([a-z_][a-z_0-9]*)")

# A field. `name: Type`, at the start of a line, which is how a struct body is
# written here — and also how a `let` annotation and a fn parameter are, so this
# reads wider than "field". That is the safe direction: see the module header.
FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z_0-9]*)\s*:\s*[^=:]")

# The comment forms. `///` and `//!` are documentation; `//` is not, and is
# exactly where rustdoc cannot help — see the module header.
COMMENT = re.compile(r"//")


def crate_of(path: Path) -> str:
    """The crate a file belongs to: the directory under `crates/` or `examples/`."""
    rel = path.relative_to(ROOT).parts
    return f"{rel[0]}/{rel[1]}" if len(rel) > 1 else rel[0]


def sources() -> list[Path]:
    out: list[Path] = []
    for tree in TREES:
        base = ROOT / tree
        if base.is_dir():
            out.extend(p for p in base.rglob("*.rs") if "target" not in p.parts)
    return sorted(out)


def comment_text(line: str) -> str:
    """Whatever on this line is a comment, or the empty string.

    ⚠ Naive about a `//` inside a string literal, and that is the safe
    direction: it can only ADD text to search, so a citation is never missed
    because of it — and a false hit would have to be a real `Type::method` in
    backticks inside a string, which is itself a citation worth checking.
    """
    hit = COMMENT.search(line)
    return line[hit.start():] if hit else ""


def survey() -> tuple[dict[str, set[str]], dict[str, set[str]], list[tuple[Path, int, str, str]]]:
    """Declared types, defined fns and every citation — one pass over the tree."""
    declares: dict[str, set[str]] = defaultdict(set)  # type name -> crates
    defines: dict[str, set[str]] = defaultdict(set)  # crate -> fn names
    citations: list[tuple[Path, int, str, str]] = []
    for path in sources():
        crate = crate_of(path)
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            found = DECLARED.match(line)
            if found:
                declares[found.group(1)].add(crate)
            for name in DEFINED.findall(line):
                defines[crate].add(name)
            member = FIELD.match(line)
            if member:
                defines[crate].add(member.group(1))
            prose = comment_text(line)
            if prose:
                for kind, method in CITED.findall(prose):
                    citations.append((path, number, kind, method))
    return declares, defines, citations


def judge() -> tuple[list[str], int, int, int]:
    """The refusals, and the population they were drawn from."""
    declares, defines, citations = survey()
    refusals: list[str] = []
    in_scope = 0
    for path, number, kind, method in citations:
        owners = declares.get(kind)
        if not owners:
            continue  # not a type this tree declares — not ours to check
        in_scope += 1
        # ★ Two crates may declare one type name, and prose in a third may cite
        # either. Measured at R2000: `Container`, `Layout`, `Selection`,
        # `Affine` and `Encoding` all name two things here. So the crate the
        # PROSE lives in wins when it declares the name — that is the one the
        # writer had in front of them — and only otherwise is the union asked.
        home = crate_of(path)
        owners = {home} if home in owners else owners
        known = set().union(*(defines[owner] for owner in owners))
        if method not in known:
            where = path.relative_to(ROOT)
            refusals.append(
                f"{where}:{number}: `{kind}::{method}` names no fn in "
                f"{', '.join(sorted(owners))} — the prose cites a name the tree "
                f"does not have"
            )
    return refusals, len(citations), in_scope, len(defines)


def selftest() -> int:
    """★ The gate's own claims, performed.

    Three, and the third is the one that matters: a gate that examines nothing
    passes for the wrong reason. R1964 and R1970 both wrote down that an
    assertion with no path to failure should be deleted; the repair for one with
    no POPULATION is to assert the population.
    """
    declares, defines, citations = survey()
    failures: list[str] = []

    if "Document" not in declares:
        failures.append("selftest: the survey found no `Document` type, so it read nothing")
    known = set().union(*(defines[owner] for owner in declares.get("Document", set())))
    for verb in ("turn", "may_turn", "retarget"):
        if verb not in known:
            failures.append(f"selftest: `Document::{verb}` is in the tree but the survey missed it")

    # The denominator. Measured at R2000: 3,000+ citations, 1,900+ in scope.
    # A floor rather than the number, because the number moves every round —
    # what must not move is that there IS one.
    _, seen, in_scope, crates = judge()
    if seen < 500 or in_scope < 200:
        failures.append(
            f"selftest: the population collapsed ({seen} citation(s), {in_scope} in scope) — "
            "this gate would pass vacuously"
        )
    if crates < 5:
        failures.append(f"selftest: only {crates} crate(s) contributed names")

    # ★ And it must REFUSE something it should refuse. A gate nobody has seen
    # say no is a gate nobody has tested.
    probe = "Document::a_verb_this_tree_does_not_have"
    kind, method = probe.split("::")
    if method in set().union(*(defines[owner] for owner in declares.get(kind, {""}))):
        failures.append(f"selftest: the probe name `{probe}` unexpectedly exists")

    for line in failures:
        print(line, file=sys.stderr)
    if failures:
        return 1
    print(
        f"prose api names: selftest ok — {seen} citation(s), {in_scope} in scope, "
        f"{crates} crate(s)"
    )
    return 0


def standing() -> set[str]:
    """The citations the pin admits, as `crate/path:`Type::member`` keys.

    ⚠ Line numbers are deliberately NOT in the key: prose moves when the file
    above it is edited, and a pin keyed by line would refuse a round that
    touched nothing it names.
    """
    if not PIN.exists():
        return set()
    return set(json.loads(PIN.read_text(encoding="utf-8"))["standing"])


def key(line: str) -> str:
    """One refusal reduced to what the pin holds: the file and the citation."""
    where, _, rest = line.partition(":")
    citation = re.search(r"`[^`]+`", rest)
    return f"{where} {citation.group(0) if citation else rest.strip()}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="refuse on any drift from the pin")
    parser.add_argument("--census", action="store_true", help="print the population and stop")
    parser.add_argument("--write", action="store_true", help="re-pin what stands today")
    parser.add_argument("--selftest", action="store_true", help="check this gate's own claims")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    refusals, seen, in_scope, crates = judge()
    found = {key(line) for line in refusals}

    if args.census:
        print(f"citations in comments: {seen}")
        print(f"  of types this tree declares: {in_scope}")
        print(f"  crates contributing fn names: {crates}")
        print(f"  naming nothing: {len(refusals)}")
        print(f"  pinned as standing: {len(standing())}")
        return 0

    if args.write:
        PIN.write_text(
            json.dumps(
                {
                    "note": (
                        "Citations in this tree's comments that name nothing. R2000 pinned "
                        "these rather than fixing them in one round: each is a separate "
                        "reading of what the writer meant. The gate demands a BIJECTION, so "
                        "repairing one refuses until this file is re-written — which is what "
                        "makes the list shrink instead of merely not growing."
                    ),
                    "standing": sorted(found),
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"prose api names: pinned {len(found)} standing citation(s)")
        return 0

    pinned = standing()
    fresh = sorted(found - pinned)
    stale = sorted(pinned - found)

    for line in refusals:
        if key(line) in fresh:
            print(line, file=sys.stderr)
    if fresh:
        print(
            f"\nprose api names: {len(fresh)} NEW citation(s) name nothing.\n"
            "A cited name is a promise a reader will follow. Rename the citation to "
            "what shipped, or delete it.",
            file=sys.stderr,
        )
    if stale:
        # ★ The half a "not growing" ratchet does not have, and the reason this
        # is a bijection: a repaired citation must be taken OFF the pin, or the
        # list would only ever record a high-water mark. `--write` does it.
        print(
            f"prose api names: {len(stale)} pinned citation(s) no longer stand — "
            "re-pin with `python3 tools/prose_api_names.py --write`:",
            file=sys.stderr,
        )
        for line in stale:
            print(f"  {line}", file=sys.stderr)

    if fresh or stale:
        return 1 if args.check else 0
    print(
        f"prose api names: {in_scope} in-scope citation(s); "
        f"{len(pinned)} standing, exactly as pinned"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
