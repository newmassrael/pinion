#!/usr/bin/env python3
"""R1646 — the capability census for the analysis-tool axis, computed rather than remembered.

## Why this exists

The node-graph axis has a meter: `tools/reference_census.py` enumerates two
reference trees and `docs/reference-census.json` carries one verdict per
operator, so its coverage number is something `cargo test` re-derives. The
**analysis-tool** axis had no such thing — its state lived in a prose table in
a session note, and prose drifts. Measured, twice in one session:

* the table said the charting remainder was "3D only", and label thinning was
  missing from it (a round had recorded the gap and a later re-audit walked past
  it);
* it listed the two-layer link diff as closed, when what existed was the
  **paint** (a dashed stroke) and the model was inside an example that did not
  depend on the crate at all.

Both are the same failure the reference census exists to prevent: a completion
nobody checked is not a measurement.

The capability list this measures is deliberately **generic**. Every row is a
property of the tool *class* — a capture/decode viewer, a node authoring and
execution lab, a widget dashboard — at the level the well-known open tool in
each of those three families describes itself publicly. No product, domain,
protocol or customer appears here, and none may; nor does any other project's
name, which the push gate enforces and caught on this file's first draft.

## The verdict vocabulary, and why five words rather than two

A framework census that answers only yes/no lies in both directions. Most of a
tool like this is neither present nor missing *in the framework*: it is the
application's own subject matter sitting on substrate that is present. So:

| verdict      | means                                                          |
|--------------|----------------------------------------------------------------|
| `have`       | the framework provides it; `covered_by` names what              |
| `gap`        | the framework must build it, and has not                        |
| `app`        | the substrate is here; the domain logic is the application's    |
| `outside`    | the SPEC excludes it; `outside_because` cites the ratified line  |
| `unmeasured` | **nobody has checked**, which is not the same as `have`         |

`unmeasured` is the point of the whole file. A row nobody has looked at is
invisible in a prose summary and is reported here, because the error direction
that matters is the silent one (R1602: a wrong `absent` self-corrects when the
next round reaches for it; a wrong `have` inflates a number nobody trips over).

## What this proves and what it does not

Completeness, and — since R1771 — that every proof it cites is real. Every
declared capability carries a verdict, the verdicts are from the closed set
above, `have` rows name what covers them, and the counts sum to the row count.
Whether a `have` is TRUE is a separate question, and the answer to it is the same
as the reference census's: a test that exercises the capability through the
public API. `proven_by` is where that goes, and how many rows carry one is
reported rather than hidden.

★★★★★ R1771 — and what a `proven_by` names is now **checked against the test
runner**. It was checked against nothing: `assembled_by` had had every path it
names verified since R1648, while the field carrying this file's STRONGEST
claim went unexamined, so a citation to a renamed or deleted test passed
silently on the rows a reader trusts most. `--check-proofs` asks
`cargo test -- --list` whether each cited name is a test that can be SELECTED —
not whether the string appears in the source, because a census that checked its
proofs with a search would be breaking its own rule in the act of enforcing it,
and a search also cannot tell a test that runs from one behind a `cfg` this
build does not enable. Measured the round it was written: three of the eleven
citations added that same round named the wrong crate, and the gate caught all
three before they were committed.

### Why this and `reference_census.py` stay two designs (R1771, obligation 3b)

The sibling census validates `proven_by` too, and has since before this one did
— which is the sharper half of what R1771 found: the two censuses in this tree
DISAGREED about whether a proof needs checking, while this file's own prose said
its answer was *the same as the reference census's*.

They are not merged, and the reason is written here so the next round does not
merge them. That tool checks the ADDRESS (`<crate>::<test>`, and that the crate
has a proof file) and says in as many words that whether the named test exists is
"deliberately only half" — deferred to a per-crate bijection test where **the
compiler** is the reader, because asking it there would be a census over text.
That is a sound design and a stronger one where it applies: a compiler cannot be
fooled by a stale string.

It does not apply here. This pin's citations are not one address per row — they
are sentences naming several tests across several crates, in three forms, beside
the demo files that drive them end to end, because a capability is rarely one
test. There is no single crate to hold a bijection for a row like that. So this
file answers the deferred half by a third route that is also not a text census:
it asks the thing that runs the tests.

## Two verdicts, two kinds of evidence (R1648)

`proven_by` cannot answer for an `app` row, and R1646 registered that as debt
without deciding what could. An `app` says *the substrate is here and the domain
logic is the application's* — a claim about COMPOSITION — and no unit test
exercises a composition. What does is a composite: an application that actually
assembles the capability out of the pieces this project ships, and a demo that
drives it.

So `app` rows carry **`assembled_by`** instead, naming the example and the demo
script, and every path it names must exist. The asymmetry is deliberate and the
two fields are mutually exclusive by verdict: a `have` is proven by a test, an
`app` by an assembly, and a row that carried the wrong kind would be claiming
evidence of a sort its verdict cannot produce.

The error direction is the same one R1602 recorded, which is why the count is
reported rather than assumed: an `app` nobody has assembled is indistinguishable
from an `app` somebody assembled, and it is the *unassembled* ones that quietly
hold the "the framework owes N" figure down.

Usage:
    python3 tools/analyzer_census.py             # the report
    python3 tools/analyzer_census.py --selftest     # the tool's own tests
    python3 tools/analyzer_census.py --check-pin    # completeness only, for a hook
    python3 tools/analyzer_census.py --check-proofs # every cited proof runs (R1771)
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PIN = ROOT / "docs" / "analyzer-census.json"

#: The spec SSOT an `outside_because` cites. Tracked, so a fresh clone can run
#: the check with no tool built and no network — which is why the citation
#: resolves against this file rather than against `mnemosyne-cli`.
STORE = ROOT / "docs" / ".atomic" / "workspace.atomic.json"

#: The closed verdict vocabulary. Spelled once; the pin is checked against it.
VERDICTS = ("have", "gap", "app", "outside", "unmeasured")

#: Verdicts that count as "the framework owes something here".
OWED = ("gap", "unmeasured")

#: The planes, in the order a reader meets them.
PLANES = ("capture", "lab", "dashboard")

#: The tiers, from "the tool does not exist without it" outward.
TIERS = ("t0", "t1", "t2")


class Finding(Exception):
    """A malformed pin. Raised rather than returned: a census that silently
    skips a bad row reports fewer rows than were declared, which is the false
    negative this file exists to make impossible."""


def load(text: str) -> list[dict]:
    """Parse and validate the pin. Pure in `text`, and strict."""
    rows = json.loads(text)
    if not isinstance(rows, list):
        raise Finding("the pin is a list of capability rows")
    seen: set[str] = set()
    for at, row in enumerate(rows):
        where = f"row {at}"
        for key in ("id", "plane", "tier", "capability", "verdict"):
            if not row.get(key):
                raise Finding(f"{where}: no {key}")
        where = f"{row['id']}"
        if row["id"] in seen:
            raise Finding(f"{where}: declared twice")
        seen.add(row["id"])
        if row["plane"] not in PLANES:
            raise Finding(f"{where}: unknown plane {row['plane']!r}")
        if row["tier"] not in TIERS:
            raise Finding(f"{where}: unknown tier {row['tier']!r}")
        if row["verdict"] not in VERDICTS:
            raise Finding(f"{where}: unknown verdict {row['verdict']!r}")
        # A `have` with nothing named is the shape a prose summary produces and
        # this file exists to refuse.
        if row["verdict"] == "have" and not row.get("covered_by"):
            raise Finding(f"{where}: verdict have and nothing named as covering it")
        # And the mirror: a verdict out of the numerator must not carry
        # evidence for one.
        if row["verdict"] in OWED and row.get("proven_by"):
            raise Finding(f"{where}: verdict {row['verdict']} and still names a proof")
        # R1648 — the two kinds of evidence do not cross. A `have` is proven by
        # a test and an `app` by an assembly; a row carrying the other kind is
        # claiming evidence of a sort its verdict cannot produce, which reads as
        # stronger than it is.
        if row.get("assembled_by") and row["verdict"] != "app":
            raise Finding(
                f"{where}: verdict {row['verdict']} names an assembly — only `app` is"
                " a claim about composition"
            )
        if row.get("proven_by") and row["verdict"] == "app":
            raise Finding(
                f"{where}: verdict app names a proof — an `app` is a claim about"
                " composition, and no unit test exercises one; use assembled_by"
            )
        # ★ R1847 — the third kind of evidence, and it does not cross either.
        # `rests_on` is the substrate half of an `app` claim; a `have` or a
        # `gap` naming one would be saying something its verdict does not mean.
        if row.get("rests_on") and row["verdict"] != "app":
            raise Finding(
                f"{where}: verdict {row['verdict']} names substrate — `rests_on`"
                " is what an `app` claims is already there"
            )
        if row.get("rests_on") and not rests_on_citations(row):
            raise Finding(
                f"{where}: rests_on names no path — the field exists to be"
                " checked, and prose with no citation in it cannot be"
            )
        # ★★★★★ R1847 — a fragment is ONE WORD. Written `foo.rs#pub fn observe`
        # the split leaves the symbol `pub`, which is in every Rust file: the
        # citation reads as precise and checks nothing. Refused here rather than
        # truncated silently, because the silent version is what shipped in this
        # round's first draft and passed.
        for path, symbol in rests_on_citations(row):
            if symbol in KEYWORDS:
                raise Finding(
                    f"{where}: rests_on names the keyword {symbol!r} in {path} —"
                    " a citation written `foo.rs#pub fn observe` splits at the"
                    " space and checks `pub`, which every Rust file has"
                )
        if not row["id"].startswith(f"{row['plane']}."):
            raise Finding(f"{where}: an id is <plane>.<tier>.<n>")
    return rows


def cited_paths(field: str) -> list[str]:
    """The repo-relative paths a citation field names.

    Pure, and deliberately loose about the prose around them: a token is a path
    if it contains a separator, so the field can read as a sentence and still be
    checkable. The alternative — a list field — makes the reason unwriteable,
    and a citation with no reason is what R1611 spent a round unwinding.

    ★★★★★ R1827 — **but a token must be repo-relative and have something on
    both sides of every separator**, and that is not fussiness. Looseness is
    safe only while a non-citation FAILS: a word that is not a path gets asked
    of the filesystem, is absent, and says so. An ABSOLUTE token is the one
    shape that can pass while naming nothing in this repository, because the
    oracle these feed is `os.path.exists` and the filesystem root exists.
    Measured in the round that added this: the prose `` `answers` /
    `answered by` `` put a bare `/` into `capture.t1.9`, `assembly_paths`
    returned it as a cited path, and `check_assemblies` confirmed it — a
    phantom citation, admitted silently, in the field whose whole job is to
    make a claim checkable. The other absolute shape this repo's prose is full
    of is an RPC path (`/external/correlation`), which would be asked of the
    filesystem and reported missing: loud, and equally wrong.

    So the rule is: relative, and no empty segment. Both extractors share this
    one function rather than spelling it twice, for the reason `cell_texts` and
    `row_cells` in this round's other half share theirs — two spellings of one
    rule is the shape that drifts.
    """
    out: list[str] = []
    for token in str(field).split():
        path = token.strip(",;")
        if "/" not in path or path.startswith("/"):
            continue
        if any(segment == "" for segment in path.split("/")):
            continue
        out.append(path)
    return out


def assembly_paths(row: dict) -> list[str]:
    """The paths an `assembled_by` names. See [`cited_paths`] for the rule."""
    return cited_paths(row.get("assembled_by", ""))


def rests_on_citations(row: dict) -> list[tuple[str, str]]:
    """The `(path, symbol)` pairs a `rests_on` names; `symbol` may be empty.

    ★★★★★ R1847 — the field that makes an `app` verdict's OTHER half checkable.
    `assembled_by` answers *has anyone composed this yet*; it says nothing about
    the claim the verdict actually makes, which is **the substrate is already
    here and only the domain logic is the application's**. A row whose substrate
    is NOT here is not an `app` at all — it is a `gap`, and `gap` is the bin the
    framework's owed number is counted from. That is the expensive direction:
    a wrong `gap` self-corrects the moment somebody reaches for it, and a wrong
    `app` deflates the owed number in silence.

    A citation is `crates/…/foo.rs#symbol`, or a bare path. The fragment is what
    makes this stronger than an existence check on a file — a module can survive
    while the item the verdict rests on is renamed away.

    ★★★★★ THE SYMBOL IS ONE WORD, AND THAT IS A RULE THIS FUNCTION LEARNED THE
    HARD WAY. The first draft split the whole field on whitespace and then cut
    at `#`, so a citation written `foo.rs#pub fn observe` yielded the symbol
    `pub` — which occurs in every Rust file in the tree. The check passed, the
    pin's six citations all had that shape, and **the gate was vacuous on the
    very rows it was built for**. It was caught by trying to make it FAIL:
    renaming the item to something absent still produced no finding. ⇒ a gate
    nobody has watched fail is a gate nobody has tested.

    So a fragment carrying a space is refused at load rather than quietly
    truncated, and the symbol is matched as a DECLARATION (`fn x`, `struct x`,
    `trait x`, …) rather than as a substring — the boundary rule this repo
    already writes down for censuses, because an unbounded substring credits
    another item's work.

    ⚠ The limit, stated rather than left to be discovered: this answers "an item
    with this name is declared in this file", not "it has this signature" or
    "the verdict needs exactly this". Reading a source file for a declaration is
    a sound use of search, which is why this is not the mistake R1771 refuses:
    THAT rule is about proving a test RUNS, where search cannot see past `cfg`
    and the oracle has to be the test runner. A declaration has no such hidden
    state.
    """
    out: list[tuple[str, str]] = []
    for token in str(row.get("rests_on", "")).split():
        cite = token.strip(",;")
        path, _, symbol = cite.partition("#")
        if not cited_paths(path):
            continue
        out.append((path, symbol))
    return out


#: How an `outside_because` names the ratified sentence that excludes a row.
#:
#: ★★★★★ R1883 — `§<id>:<phrase>`, where `<id>` is a section of the atomic store
#: and `<phrase>` is a literal substring of that section's DECISION TEXT. The
#: shape is deliberately the one `analyzer_debts.py` already uses for
#: `standing_because: body:<phrase>` — a citation a reader can resolve in one
#: step, against a document rather than against this file's own prose.
BOUNDARY = re.compile(r"^§([0-9]+(?:\.[0-9]+)*):\s*(\S.*)$", re.S)


def boundary_citation(row: dict) -> tuple[str, str] | None:
    """The `(section, phrase)` an `outside_because` names, or `None`.

    `None` covers both "the field is absent" and "the field is not a citation",
    and the caller distinguishes them — an absent field and a malformed one are
    different findings and a reader needs to be told which.
    """
    found = BOUNDARY.match(str(row.get("outside_because", "")).strip())
    return (found.group(1), found.group(2).strip()) if found else None


def check_boundaries(rows: list[dict], sections: dict[str, tuple[str, str]]) -> list[str]:
    """Every `outside` verdict that cannot name the ratified line excluding it.

    ★★★★★ R1883 — **`outside` is the strongest claim in this vocabulary and it
    was the only one that had to earn nothing.** A `have` names a test, an `app`
    names an assembly or the substrate it rests on, a `gap` is a debt that
    self-corrects the moment somebody reaches for it. `outside` does more than
    any of them: it takes the row OUT OF THE POPULATION — not owed, not
    evidenced, not counted — and until this check the word `outside` appeared in
    exactly two places in this file, the doc table above and `VERDICTS`. Nothing
    read `covered_by` on such a row; nothing could.

    That is the escape hatch defanging its own gate. The census exists to stop a
    verdict being one person's unreviewed reading, and the bin that removes a
    row from the count entirely was the bin nobody had to justify.

    # Why the citation resolves against the STORE and not against a list here

    A closed vocabulary spelled in this file would make this tool the second
    author of the boundary, and the boundary is ratified elsewhere — §3, whose
    codec clause has already been AMENDED once (the audio half retired, §5.54).
    A hand-copy would have gone on excluding audio while the spec had stopped.
    So the phrase must be a literal substring of the section's own decision text
    and the tool holds no copy of it.

    ⚠ **Decision text is `intent` + `caveats_bullets`, and NOT
    `alternatives_rejected`** — those are the options the round considered and
    turned down. A row citing a rejected alternative would be citing something
    the spec declined to adopt, which reads like a boundary and is its opposite.

    ⚠ **A ratified sentence is not always quotable into a tracked file, and
    this round found that by tripping it.** The citation is a literal substring
    of the spec, and §3's browser-engine clause NAMES the product it excludes —
    so writing that bullet verbatim into the pin (or into a round's prose) is
    refused by `tools/reference_names.py`, the confidentiality ratchet. The
    citation is still resolvable: §3 carries other clauses, and a row whose only
    honest ground IS that one should cite the shortest span of it that carries
    no product name. Stated here rather than left to be rediscovered, because
    the two gates are enforced by different hooks and neither mentions the other.

    ⚠ The section must be `active`. Measured at R1883 that refuses nothing —
    all 75 sections are active — so it is a property rather than a branch this
    pin exercises, and the selftest is what exercises it. It is here because a
    superseded section's sentence is not a ratified boundary, and a check that
    only holds while the store happens to be uniform is not a check.

    Pure in `rows` and `sections`, like its three siblings, so every rule is
    testable without a filesystem, a toolchain or the 17 MB store.
    """
    out: list[str] = []
    for row in rows:
        verdict, ident = row["verdict"], row["id"]
        if verdict != "outside":
            # Evidence is exclusive per verdict here as everywhere else in this
            # file: a claim carrying the wrong KIND of evidence reads stronger
            # than it is, and `load` already refuses a `have` that names an
            # assembly. This is that rule for the field this round adds.
            if str(row.get("outside_because", "")).strip():
                out.append(
                    f"{ident}: `outside_because` is evidence for an `outside` "
                    f"verdict and this row is `{verdict}`"
                )
            continue
        raw = str(row.get("outside_because", "")).strip()
        if not raw:
            out.append(
                f"{ident}: an `outside` must cite the ratified line that excludes "
                "it (outside_because is empty) — `covered_by` is prose and no "
                "gate can read it"
            )
            continue
        cited = boundary_citation(row)
        if cited is None:
            out.append(
                f"{ident}: `outside_because: {raw[:60]}` is not `§<section>:<phrase>`"
            )
            continue
        section, phrase = cited
        held = sections.get(section)
        if held is None:
            out.append(f"{ident}: cites §{section}, which is not a section of the store")
            continue
        status, text = held
        if status != "active":
            out.append(
                f"{ident}: cites §{section}, whose decision_status is "
                f"`{status}` — a superseded section ratifies nothing"
            )
        elif phrase not in text:
            out.append(
                f"{ident}: cites §{section} for a sentence it does not contain — "
                f"{phrase[:60]!r}"
            )
    return out


#: Words that are never the NAME of an item, only the grammar around one.
#:
#: ★★★★★ R1847 — the guard on the shape this round shipped in its own first
#: draft. `rests_on` is split on whitespace before the `#` is cut, so a citation
#: written `foo.rs#pub fn observe` yields the symbol `pub` — present in every
#: Rust file, so the check passes and the gate is vacuous exactly where it was
#: needed. The space itself cannot be seen after the split; what CAN be seen is
#: that the surviving word is grammar rather than a name, and that is what this
#: refuses. A citation naming an item is unaffected.
KEYWORDS: frozenset[str] = frozenset(
    {
        "pub",
        "fn",
        "struct",
        "trait",
        "enum",
        "const",
        "static",
        "type",
        "mod",
        "impl",
        "use",
        "let",
        "crate",
        "macro_rules!",
    }
)

#: How a Rust item is introduced, for [`check_rests`]'s declaration match.
DECLARES: tuple[str, ...] = (
    "fn",
    "struct",
    "trait",
    "enum",
    "const",
    "static",
    "type",
    "mod",
    "macro_rules!",
)


def declares(text: str, symbol: str) -> bool:
    """Whether `text` DECLARES `symbol`, rather than merely containing it.

    Pure, and deliberately narrow: `<keyword> <symbol>` followed by a character
    that cannot continue an identifier. `parse` therefore does not answer for
    `parse_header`, which is the failure an unbounded substring produces.
    """
    for keyword in DECLARES:
        at = 0
        needle = f"{keyword} {symbol}"
        while (at := text.find(needle, at)) != -1:
            after = at + len(needle)
            tail = text[after] if after < len(text) else " "
            if not (tail.isalnum() or tail == "_"):
                return True
            at = after
    return False


def check_rests(rows: list[dict], exists, read) -> list[str]:
    """Every substrate a `rests_on` names that the tree cannot answer for.

    Pure in `exists` and `read`, like its two siblings, so the rule is testable
    without a filesystem.
    """
    out: list[str] = []
    for row in rows:
        for path, symbol in rests_on_citations(row):
            if not exists(path):
                out.append(f"{row['id']}: rests_on names {path}, which is not there")
                continue
            if symbol and not declares(read(path), symbol):
                out.append(
                    f"{row['id']}: rests_on names {symbol} in {path},"
                    " which declares no such item"
                )
    return out


def check_assemblies(rows: list[dict], exists) -> list[str]:
    """Every path an `assembled_by` names, that `exists` says is not there.

    Pure in `exists` so the rule is testable without a filesystem. The check
    matters because the failure it catches is silent: an assembly that was
    renamed or deleted leaves the row still claiming to be composed, and the
    verdict it supports is the largest and least reviewed bin in the file.
    """
    return [
        f"{row['id']}: assembled_by names {path}, which is not there"
        for row in rows
        for path in assembly_paths(row)
        if not exists(path)
    ]


#: Rows allowed to carry an `app` verdict with no `assembled_by`.
#:
#: ★★★★★ R1807 — a RATCHET, not an exemption. `check_evidence` refuses any row
#: whose verdict promises evidence and carries none; this names the ones that
#: already did when the rule landed, so a NEW claim cannot join them silently
#: and the backlog is a list a reader can shorten rather than a number in a
#: summary line.
#:
#: `have` has no such list on purpose. R1807 took its backlog to zero in the
#: round that built the rule, which is the only moment a ratchet can start
#: empty — and an empty allowlist is the one that cannot rot.
UNASSEMBLED: frozenset[str] = frozenset(
    {
        # `capture.t2.14` was here and is REPAID at R1852 — the capture builds a
        # topology out of the hops it has always carried, and the entry
        # re-measurement found the row's own `rests_on` pointing at the wrong
        # half: `observe` records traffic between sockets that ALREADY EXIST, so
        # it presupposes the drawing a capture does not have.
        # `pinion_node_graph::SightedTopology` is that missing half, shaped by
        # what a sightings-only graph may not say — `not seen` is not `no such
        # link`, an isolated endpoint is not expressible, and the standing is
        # never certain.
        # `capture.t2.18` was here and is REPAID at R1845 — the capture derives
        # its own protocol violations, against a watermark rather than an
        # adjacent difference, and the reassembly strip's lanes are the same
        # reading, so a lane and a violation row cannot disagree.
        # `dashboard.t1.8` was here and is REPAID at R1843. Removing a name is
        # the only way this list shortens, and the ratchet refuses both
        # directions: a row that gains an `assembled_by` while still listed
        # here fails by name, so the name and the field cannot drift apart.
        # `dashboard.t1.9` was here and is REPAID at R1851 — an alarm feed is a
        # virtualised list under a sortable severity header, and R1851 measured
        # what its verdict had been standing on: both halves existed and NOTHING
        # composed them. `pinion_widget_paint::header_feed` is that join, and it
        # exists to stop three facts being read twice — the indicator IS the
        # order, the body's viewport is the rect minus the header, and the window
        # a reader is told about is the window that was built.
        # `lab.t1.8` was here and is REPAID at R1848 — a traffic role declares
        # which of a closed vocabulary its nodes carry, and a card's stated and
        # unstated parameters are derived from that declaration and its own
        # rows. ★ The taxonomy was worth declaring because the screen already
        # PAINTED these parameters as free-text rows with nothing saying which
        # keys belong to a role, so "does this node state its priority?" had
        # nowhere to be asked.
        # `lab.t1.9` was here and is REPAID at R1844 — the scenario gained a
        # fifth act that ASKS rather than commands, with the timeout that makes
        # it an assertion about an interval instead of a sample of one instant.
        # `lab.t1.11` was here and is REPAID at R1853 — a fault-injection panel
        # whose offers are DERIVED from the target's declared settings and
        # confirmed twice before being offered, with the faults it cannot reach
        # named on screen by filtering the boundary rather than written out.
        # ★ The row's covering sentence had been one word, `a form`, and the gap
        # under it was real: a form can HOLD a bad value and nothing could
        # enumerate which bad values a declaration admits.
        # ★★★★★ And the round's own first derivation was refuted mid-round —
        # `ConfigForm::adopt` reports an undeclared leaf as unplaceable, so *a
        # key the target does not know* is a fault a form cannot reach. That is
        # why `Scope` has three arms instead of two.
        # `lab.t2.17` was here and is REPAID at R1866 — two runs of one graph
        # compared on order and latency, and the entry re-measurement found the
        # MIDDLE of its sentence missing rather than either end: a run's step
        # says which node ran and not when, so there were no amounts for the
        # chart crate's distributions to summarise. `pinion_core::regression`
        # is that join, and its shape is the finding — an order and a latency
        # profile are one axis at two scales, so one comparison answers both,
        # and two timelines at different scales are refused because a shift of
        # two is two steps or two seconds and the number does not say which.
        "lab.t2.19",
    }
)


def check_evidence(rows: list[dict]) -> list[str]:
    """Every row whose verdict PROMISES evidence and carries none.

    ★★★★★ R1807 — the hole under both evidence checks. `check_proofs` iterates
    the citations a `proven_by` makes, and `check_assemblies` the paths an
    `assembled_by` names; a row carrying NEITHER field iterates zero times and
    passes both. So the census refused a citation that had rotted and admitted a
    `have` that never cited anything — the stronger claim, checked less. It was
    visible only as prose in the report ("28 of 35 name a proof — the rest are
    claims, and this says so"), which is a sentence a reader can agree with and
    no gate can act on.

    That is this project's recurring shape: a rule stated where it cannot be
    enforced. `--check-proofs` was itself built (R1771) to end exactly this for
    `proven_by`, and stopped one step short — it made the citations answerable
    without making them REQUIRED.

    Pure in `rows`, like its two siblings, so the rule is testable without a
    toolchain or a filesystem.
    """
    out: list[str] = []
    for row in rows:
        verdict, ident = row["verdict"], row["id"]
        if verdict == "have" and not proof_names(row) and not proof_paths(row):
            out.append(
                f"{ident}: a `have` must name a test that exercises it "
                "(proven_by is empty)"
            )
        if verdict == "app" and not assembly_paths(row) and ident not in UNASSEMBLED:
            out.append(
                f"{ident}: an `app` must name the assembly that composes it "
                "(assembled_by is empty)"
            )
        # ★★★★★ R1847 — and the row on the ratchet owes the OTHER half. An
        # `app` nobody has composed is a claim resting on nothing a reader can
        # check: the only thing separating it from a `gap` is that the substrate
        # is already here, and until R1847 no row said where. Required exactly
        # where the assembly is absent, because a row that IS composed has an
        # example driving the substrate, which is the stronger evidence of the
        # two — demanding both there would be ceremony rather than a check.
        if verdict == "app" and ident in UNASSEMBLED and not rests_on_citations(row):
            out.append(
                f"{ident}: an `app` with no assembly must name the substrate it "
                "rests on, or it is a `gap` (rests_on is empty)"
            )
    return out


def proof_names(row: dict) -> list[str]:
    """The Rust test names a `proven_by` cites.

    ★★★★★ R1771 — the peer of [`assembly_paths`], and it did not exist. That
    asymmetry is the defect this function is half of: `assembled_by` has had
    every path it names checked for existence since R1648, and `proven_by` — the
    field carrying the census's STRONGEST claim, that a capability was exercised
    through the public API — was checked for nothing at all. A citation to a
    test that was renamed, moved or never written passed silently, and the rows
    it supports are the ones a reader trusts most.

    Loose about the prose around the names, for the reason its sibling is: the
    field has to read as a sentence or the reason goes unwritten. A token counts
    as a citation when it is a `::` path (`crate::module::test`) or a bare
    snake_case identifier long enough not to be a word — measured over this
    pin, the two forms in use are `pinion-node-graph::r1645_…` and a file path
    followed by bare names.

    The `::` form's own tail is what is checked, because a citation names a
    CRATE and this project's crate names are not module paths (`pinion-core::`
    is not a segment of `widgets::field_bytes::tests::…`). A module citation —
    a `::` path whose tail is not itself a test — is checked as a prefix by
    [`check_proofs`], so `…::row_query::tests` means *the tests under here*.
    """
    out: list[str] = []
    for token in str(row.get("proven_by", "")).split():
        word = token.strip(",;.+()")
        if "/" in word:
            continue  # a path — `check_proofs` hands those to the path oracle
        if "::" in word:
            out.append(word)
        elif "_" in word and len(word) >= 12 and word.replace("_", "").isalnum():
            out.append(word)
    return out


def proof_paths(row: dict) -> list[str]:
    """The files a `proven_by` cites — the end-to-end demos beside the tests.

    Same rule as its peer, out of the same function: see [`cited_paths`].
    """
    return cited_paths(row.get("proven_by", ""))


def check_proofs(rows: list[dict], known, exists) -> list[str]:
    """Every proof a `proven_by` names that nothing in the tree answers for.

    `known` is asked for a test name and answers whether the TEST RUNNER can
    select it — not whether the string appears somewhere. That distinction is
    the whole point: this project's rule is *prove it with a test, not with a
    grep*, and a census that checked its proofs by searching source text would
    be breaking that rule in the act of enforcing it.
    `exists` answers for the demo files cited beside the tests.

    Both are injected so the rule is testable without a toolchain, the same
    purity [`check_assemblies`] has.
    """
    out: list[str] = []
    for row in rows:
        for name in proof_names(row):
            if not known(name):
                out.append(f"{row['id']}: proven_by names {name}, which no test answers to")
        for path in proof_paths(row):
            if not exists(path):
                out.append(f"{row['id']}: proven_by names {path}, which is not there")
    return out


def report(rows: list[dict]) -> list[str]:
    """The report, as lines. Pure in `rows`."""
    out: list[str] = []
    total = len(rows)
    tally = {verdict: sum(1 for r in rows if r["verdict"] == verdict) for verdict in VERDICTS}
    if sum(tally.values()) != total:
        raise Finding("the tally does not sum to the row count")
    out.append(f"analysis-tool census — {total} capability(ies)")
    out.append("")
    for plane in PLANES:
        here = [r for r in rows if r["plane"] == plane]
        counts = {v: sum(1 for r in here if r["verdict"] == v) for v in VERDICTS}
        owed = sum(counts[v] for v in OWED)
        out.append(
            f"  {plane:<10} {len(here):>3}  "
            + "  ".join(f"{v} {counts[v]}" for v in VERDICTS)
            + f"   | owed {owed}"
        )
        for tier in TIERS:
            rung = [r for r in here if r["tier"] == tier]
            if not rung:
                continue
            debt = [r for r in rung if r["verdict"] in OWED]
            out.append(
                f"    {tier}  {len(rung):>2} capability(ies), {len(debt)} owed"
                + ("" if not debt else ": " + ", ".join(r["id"] for r in debt))
            )
        out.append("")
    out.append("  " + "  ".join(f"{v} {tally[v]}" for v in VERDICTS))
    owed_rows = [r for r in rows if r["verdict"] in OWED]
    out.append(
        f"  the framework owes {len(owed_rows)} of {total}"
        f" ({len([r for r in owed_rows if r['verdict'] == 'gap'])} known,"
        f" {len([r for r in owed_rows if r['verdict'] == 'unmeasured'])} unchecked)"
    )
    proven = [r for r in rows if r["verdict"] == "have" and r.get("proven_by")]
    out.append(
        f"  {len(proven)} of {tally['have']} `have` verdict(s) name a proof —"
        " the rest are claims, and this says so"
    )
    assembled = [r for r in rows if r["verdict"] == "app" and r.get("assembled_by")]
    out.append(
        f"  {len(assembled)} of {tally['app']} `app` verdict(s) name an assembly —"
        " the rest are claims about composition nobody has composed"
    )
    rested = [r for r in rows if r["verdict"] == "app" and rests_on_citations(r)]
    out.append(
        f"  {len(rested)} of {tally['app']} `app` verdict(s) name the substrate they"
        " rest on — every one nobody has composed must, or it is a `gap`"
    )
    bounded = [r for r in rows if r["verdict"] == "outside" and boundary_citation(r)]
    out.append(
        f"  {len(bounded)} of {tally['outside']} `outside` verdict(s) cite the ratified"
        " line that excludes them — the verdict that leaves the population owes the most"
    )
    for row in owed_rows:
        out.append(f"    {row['verdict']:<10} {row['id']:<16} {row['capability']}")
    return out


def selftest() -> int:
    """The tool's own tests: every rule above, exercised."""
    failures = 0

    def check(name: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"FAIL: {name}")

    good = [
        {
            "id": "capture.t0.1",
            "plane": "capture",
            "tier": "t0",
            "capability": "x",
            "verdict": "have",
            "covered_by": "y",
        }
    ]
    check("a well-formed row loads", len(load(json.dumps(good))) == 1)

    def refuses(name: str, rows: list[dict]) -> None:
        try:
            load(json.dumps(rows))
        except Finding:
            return
        check(name, False)

    refuses("a have with nothing covering it", [{**good[0], "covered_by": ""}])
    refuses("an unknown verdict", [{**good[0], "verdict": "maybe"}])
    refuses("an unknown plane", [{**good[0], "plane": "elsewhere"}])
    refuses("an unknown tier", [{**good[0], "tier": "t9"}])
    refuses("a missing capability", [{**good[0], "capability": ""}])
    refuses("an id that disagrees with its plane", [{**good[0], "id": "lab.t0.1"}])
    refuses("a duplicate id", good + good)
    refuses(
        "a gap that names a proof",
        [{**good[0], "verdict": "gap", "covered_by": "", "proven_by": "x::y"}],
    )
    refuses("a pin that is not a list", {"id": "x"})  # type: ignore[arg-type]

    # R1648 — the two kinds of evidence do not cross, in either direction.
    refuses(
        "a have that names an assembly",
        [{**good[0], "assembled_by": "examples/x + tools/demos/y.py"}],
    )
    refuses(
        "an app that names a proof",
        [{**good[0], "verdict": "app", "covered_by": "", "proven_by": "crate::test"}],
    )
    # ★ R1847 — and the third kind does not cross either.
    refuses(
        "a have that names substrate",
        [{**good[0], "rests_on": "crates/x/src/y.rs#Thing"}],
    )
    refuses(
        "an app whose substrate is prose with no citation in it",
        [{**good[0], "verdict": "app", "covered_by": "y", "rests_on": "it is there"}],
    )
    # ★★★★★ R1847 — and the shape that shipped in this round's own first draft:
    # a fragment carrying a space checks only its first word. Written here as a
    # refusal because a counterfactual found nothing catching its removal, which
    # is how a rule comes to be believed rather than enforced.
    refuses(
        "an app whose symbol carries a space, so only its first word is checked",
        [
            {
                **good[0],
                "verdict": "app",
                "covered_by": "y",
                "rests_on": "crates/x/src/y.rs#pub fn observe",
            }
        ],
    )
    ok_app = [
        {
            **good[0],
            "verdict": "app",
            "covered_by": "",
            "assembled_by": "assembled by examples/x, driven by tools/demos/y.py",
        }
    ]
    check("an app that names an assembly loads", len(load(json.dumps(ok_app))) == 1)
    check(
        "the paths are picked out of the prose around them",
        assembly_paths(ok_app[0]) == ["examples/x", "tools/demos/y.py"],
    )
    check(
        "a missing assembly is reported by row and by path",
        check_assemblies(ok_app, lambda p: p != "tools/demos/y.py")
        == ["capture.t0.1: assembled_by names tools/demos/y.py, which is not there"],
    )
    check(
        "and a present one is not",
        check_assemblies(ok_app, lambda _p: True) == [],
    )

    # ★★★★★ R1827 — the phantom citation. A token that is not a path must not
    # be extracted as one when the filesystem would AGREE with it: `/` exists,
    # `/tmp` exists, and a row citing either would pass while naming nothing in
    # this repository. Found in this round's own `assembled_by`, where the
    # prose `` `answers` / `answered by` `` put a bare `/` in the field.
    prosey = {
        **ok_app[0],
        "assembled_by": "assembled by examples/x, which says `answers` / `answered by`",
    }
    check(
        "a bare separator in the prose is not a citation",
        assembly_paths(prosey) == ["examples/x"],
    )
    check(
        "and so the row is not confirmed by the filesystem root existing",
        check_assemblies([prosey], lambda p: p in ("/", "examples/x")) == [],
    )
    check(
        "an absolute token is not a citation either — an RPC path is not a file",
        assembly_paths({**ok_app[0], "assembled_by": "read at /external/correlation"}) == [],
    )
    check(
        "a token with an empty segment is not a citation",
        cited_paths("tools//demos/y.py examples/x/") == [],
    )
    check(
        "and the two extractors answer with one rule, not two",
        assembly_paths({"assembled_by": "a/b /c d/ e//f"})
        == proof_paths({"proven_by": "a/b /c d/ e//f"})
        == ["a/b"],
    )

    # ★★★★★ R1807 — the row that names NOTHING, which both checks above pass
    # vacuously: they iterate a field's contents, and an absent field has none.
    bare_have = [{**good[0], "verdict": "have", "covered_by": "y"}]
    check(
        "a have that cites no test at all is refused",
        [f for f in check_evidence(bare_have) if "proven_by is empty" in f] != [],
    )
    check(
        "and the two older checks let it through, which is why this exists",
        check_proofs(bare_have, lambda _n: False, lambda _p: False) == []
        and check_assemblies(bare_have, lambda _p: False) == [],
    )
    cited_have = [{**bare_have[0], "proven_by": "some_module::tests::a_real_test_name"}]
    check(
        "a have that cites a test is not refused by this rule",
        check_evidence(cited_have) == [],
    )
    bare_app = [{**good[0], "id": "capture.t9.9", "verdict": "app", "covered_by": "y"}]
    check(
        "an app that names no assembly is refused when it is not on the ratchet",
        [f for f in check_evidence(bare_app) if "assembled_by is empty" in f] != [],
    )
    # ★★★★★ R1847 — this case used to assert that a row on the ratchet is
    # refused by NOTHING, and that was the hole: the ratchet excuses the
    # ASSEMBLY, and until this round a row with neither an assembly nor a named
    # substrate was an `app` verdict resting on a sentence. The exemption is
    # narrowed to what it was for, and the case now says which half it excuses.
    on_ratchet = {**bare_app[0], "id": next(iter(UNASSEMBLED))}
    check(
        "the ratchet excuses the assembly and not the substrate",
        [f for f in check_evidence([on_ratchet]) if "assembled_by is empty" in f] == []
        and [f for f in check_evidence([on_ratchet]) if "rests_on is empty" in f] != [],
    )
    check(
        "and a row on it that names its substrate is not refused at all",
        check_evidence([{**on_ratchet, "rests_on": "crates/x/src/y.rs#Thing"}]) == [],
    )
    check(
        "the ratchet holds only app rows, so it cannot excuse a have",
        check_evidence([{**bare_have[0], "id": next(iter(UNASSEMBLED))}]) != [],
    )

    # ★★★★★ R1847 — `rests_on`, the substrate half of an `app` claim.
    rested = [{**good[0], "verdict": "app", "covered_by": "y", "rests_on": "a/b.rs#Sym"}]
    check(
        "a substrate whose file is gone is refused",
        check_rests(rested, lambda _p: False, lambda _p: "") != [],
    )
    check(
        "a substrate whose file is there but whose symbol is not is refused too",
        check_rests(rested, lambda _p: True, lambda _p: "nothing like it here") != [],
    )
    check(
        "and the pair that answers is not refused",
        check_rests(rested, lambda _p: True, lambda _p: "pub fn Sym()") == [],
    )
    # ★★★★★ R1847 — the case that separates a DECLARATION from a substring, and
    # it exists because a counterfactual found nothing holding the difference:
    # every other case here fails under either reading. `Sym` occurs in
    # `Symbol`, so a plain `in` says yes and this must say no.
    check(
        "a symbol that is only a PREFIX of a declared item does not answer for it",
        check_rests(rested, lambda _p: True, lambda _p: "pub fn Symbol() {}") != [],
    )
    check(
        "and the boundary holds on the other side too",
        not declares("fn parse_header() {}", "parse")
        and declares("fn parse(x: u8) {}", "parse"),
    )
    check(
        "a bare path with no symbol asks only that the file be there",
        check_rests(
            [{**rested[0], "rests_on": "a/b.rs"}], lambda _p: True, lambda _p: ""
        )
        == [],
    )
    check(
        "prose in the field is skipped, and prose ALONE is refused at load",
        rests_on_citations({"rests_on": "the parse is domain"}) == [],
    )

    # ★★★★★ R1883 — `outside_because`, the evidence for the verdict that leaves
    # the population. Every case is a shape a pin can actually carry, and the
    # population here IS this block: after R1883's audit the pin holds no
    # `outside` row at all, so the selftest is the only place these branches run
    # against data. That is stated rather than hidden — a gate whose real
    # population is empty is a gate this repository has been burned by (R1877),
    # and the answer is that the RULE is exercised even when the pin is not.
    spec = {
        "3": ("active", "scope excludes WebEngine and codec embed\nvideo codecs"),
        "9": ("superseded", "an old sentence nobody ratifies now"),
    }
    outside = {**good[0], "verdict": "outside", "covered_by": "prose"}
    check(
        "an outside that cites nothing is refused",
        [f for f in check_boundaries([outside], spec) if "outside_because is empty" in f]
        != [],
    )
    check(
        "an outside whose citation is not §<section>:<phrase> is refused",
        check_boundaries([{**outside, "outside_because": "a codec concern"}], spec) != [],
    )
    check(
        "an outside citing a section the store does not have is refused",
        check_boundaries([{**outside, "outside_because": "§77: anything"}], spec) != [],
    )
    check(
        "an outside citing a SUPERSEDED section is refused",
        check_boundaries(
            [{**outside, "outside_because": "§9: an old sentence"}], spec
        )
        != [],
    )
    check(
        "an outside citing a sentence its section does not contain is refused",
        check_boundaries(
            [{**outside, "outside_because": "§3: supervision is out of scope"}], spec
        )
        != [],
    )
    check(
        "and the one that cites a ratified sentence is not refused",
        check_boundaries(
            [{**outside, "outside_because": "§3: scope excludes WebEngine"}], spec
        )
        == [],
    )
    # ★ Evidence is exclusive per verdict here as everywhere else: a `have`
    # carrying a boundary citation is claiming to be excluded and measured at
    # once, which reads stronger than either.
    check(
        "a non-outside row carrying a boundary citation is refused",
        check_boundaries(
            [{**good[0], "outside_because": "§3: scope excludes WebEngine"}], spec
        )
        != [],
    )
    check(
        "and a non-outside row with no such field is untouched",
        check_boundaries([good[0]], spec) == [],
    )

    # ★★★★★ R1771 — the same rules for the OTHER kind of evidence, which had
    # none. Every case below is a shape this pin actually carries.
    ok_proof = [
        {
            **good[0],
            "proven_by": (
                "crates/pinion-core/src/widgets/field_bytes.rs "
                "the_two_directions_are_inverse_for_every_field_with_bytes + "
                "owner_is_the_last_link_of_the_layer_chain; end to end "
                "tools/demos/r1663_a_field_says_which_bytes.py section D"
            ),
        }
    ]
    check(
        "the test names are picked out of the prose around them",
        proof_names(ok_proof[0])
        == [
            "the_two_directions_are_inverse_for_every_field_with_bytes",
            "owner_is_the_last_link_of_the_layer_chain",
        ],
    )
    check(
        "★ and the prose is not — `end`, `to`, `end`, `section` and `D` are words",
        all(w not in proof_names(ok_proof[0]) for w in ("end", "to", "section", "D")),
    )
    check(
        "the files it cites are picked out too, and not counted as tests",
        proof_paths(ok_proof[0])
        == [
            "crates/pinion-core/src/widgets/field_bytes.rs",
            "tools/demos/r1663_a_field_says_which_bytes.py",
        ],
    )
    check(
        "a test nothing answers to is reported by row and by name",
        check_proofs(
            ok_proof,
            lambda n: n != "owner_is_the_last_link_of_the_layer_chain",
            lambda _p: True,
        )
        == [
            "capture.t0.1: proven_by names owner_is_the_last_link_of_the_layer_chain,"
            " which no test answers to"
        ],
    )
    check(
        "a cited file that is gone is reported too",
        len(check_proofs(ok_proof, lambda _n: True, lambda p: not p.endswith(".py"))) == 1,
    )
    check(
        "and a proof that answers everywhere is silent",
        check_proofs(ok_proof, lambda _n: True, lambda _p: True) == [],
    )

    crate_cited = [{**good[0], "proven_by": "pinion-node-graph::r1645_the_layer_is_derived"}]
    check(
        "a `crate::test` citation names its crate, so the oracle knows what to list",
        cited_crates(crate_cited) == ["pinion-node-graph"],
    )
    check(
        "and a bare name asks for no crate, because it cannot say which",
        cited_crates(ok_proof) == [],
    )

    # The oracle: three citation forms, each matched the way it is written.
    listed = {
        "widgets::field_bytes::tests::owner_is_the_last_link_of_the_layer_chain",
        "widgets::row_query::tests::a_filter_is_total_over_its_operators",
    }
    known = proof_oracle(listed)
    check("a bare name matches a test's tail", known("owner_is_the_last_link_of_the_layer_chain"))
    check(
        "a `crate::…::name` matches past the crate segment",
        known("pinion-core::widgets::field_bytes::tests::owner_is_the_last_link_of_the_layer_chain"),
    )
    check(
        "★ a MODULE citation matches the tests under it, which is what it means",
        known("pinion-core::widgets::row_query::tests"),
    )
    check(
        "and a name nothing answers to is refused",
        not known("a_test_that_was_deleted_two_rounds_ago"),
    )
    check(
        "★★ a name that is only a SUBSTRING of a real test is refused — the "
        "failure a grep-shaped oracle would let through",
        not known("owner_is_the_last_link"),
    )

    # The report sums, which is the property the capability list itself asks
    # for: a meter whose numbers do not add up is not a meter.
    mixed = [
        {**good[0], "id": f"capture.t0.{n}", "verdict": v, "covered_by": "y" if v == "have" else ""}
        for n, v in enumerate(VERDICTS)
    ]
    lines = report(load(json.dumps(mixed)))
    check("the report renders every verdict", all(v in "\n".join(lines) for v in VERDICTS))
    check("the report counts the rows", f"{len(VERDICTS)} capability(ies)" in lines[0])

    print(f"selftest: {'PASS' if not failures else 'FAIL'} ({failures} failure(s))")
    return 1 if failures else 0


#: The crates a `proven_by` may cite, derived from the pin rather than listed.
#: Deriving it is what keeps the oracle honest: a citation into a crate this
#: never asks about would be checked against a set that cannot contain it, and
#: would fail for the wrong reason.
def cited_crates(rows: list[dict]) -> list[str]:
    """The crate each `::` citation names, in the order the pin declares them."""
    out: list[str] = []
    for row in rows:
        for name in proof_names(row):
            if "::" not in name:
                continue
            crate = name.split("::", 1)[0]
            if crate not in out:
                out.append(crate)
    return out


def store_sections() -> dict[str, tuple[str, str]]:
    """Each ratified section as `(decision_status, decision text)`.

    ★★★★★ R1883 — the oracle [`check_boundaries`] resolves a citation against.
    Decision text is `intent` + `caveats_bullets` joined; `alternatives_rejected`
    is deliberately absent, for the reason that function states — a rejected
    alternative reads like a boundary and is its opposite.

    Reads the tracked store directly rather than shelling out to
    `mnemosyne-cli`: this runs on EVERY invocation of the census, a fresh clone
    has no built tool, and the file is the SSOT the tool would have read anyway.
    Measured at R1883: 17.3 MB parses in 0.11s, which is why it can be here
    beside the three checks that build nothing rather than behind a flag.

    Raises `Finding` when the store cannot be read — **fail closed**, because
    the alternative is a boundary check that silently stops happening, which is
    the failure mode every gate in this file exists to prevent.
    """
    try:
        held = json.loads(STORE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as why:
        raise Finding(f"the spec store is unreadable ({STORE.name}): {why}") from why
    out: dict[str, tuple[str, str]] = {}
    for ident, section in (held.get("sections") or {}).items():
        text = "\n".join(
            [str(section.get("intent", ""))]
            + [str(one) for one in (section.get("caveats_bullets") or [])]
        )
        out[str(ident)] = (str(section.get("decision_status", "")), text)
    return out


def runner_tests(crates: list[str]) -> set[str]:
    """Every test the TEST RUNNER can select in `crates`.

    ★ R1771 — `cargo test -- --list` rather than a search of the source, for the
    reason stated on [`check_proofs`]: the census's rule is that a capability is
    proven by a test, and the only thing that knows what tests exist is the
    thing that runs them. A regex over `#[test]` would also see a test behind a
    `cfg` this build does not enable, which is exactly a proof that does not run.

    Returns the full `module::path::name` of each, so a caller can match a bare
    name against the tail or a module citation against a prefix.
    """
    import subprocess

    if not crates:
        return set()
    argv = ["cargo", "test", "--all-targets"]
    for crate in crates:
        argv += ["-p", crate]
    argv += ["--", "--list"]
    done = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise Finding(
            "the test runner could not list this workspace's tests, so no proof "
            f"can be checked: {done.stderr.strip().splitlines()[-1:] or ['(no output)']}"
        )
    return {
        line.rsplit(":", 1)[0].strip()
        for line in done.stdout.splitlines()
        if line.endswith(": test")
    }


def proof_oracle(listed: set[str]):
    """An answer to *can the runner select this citation*, over `listed`.

    Three citation forms, and each is matched the way it is written:
    a bare name against any test's tail, a `crate::…::name` against any test
    whose path ends with the segments after the crate, and a module citation
    against any test whose path passes through it.
    """

    def known(name: str) -> bool:
        if "::" not in name:
            return any(full.rsplit("::", 1)[-1] == name for full in listed)
        tail = name.split("::", 1)[1]
        return any(full == tail or full.endswith(f"::{tail}") for full in listed) or any(
            full.startswith(f"{tail}::") or f"::{tail}::" in full for full in listed
        )

    return known


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()
    try:
        rows = load(PIN.read_text(encoding="utf-8"))
    except Finding as why:
        print(f"analyzer census: {why}", file=sys.stderr)
        return 1
    # R1648 — the assemblies an `app` row cites must be there. Run before the
    # report on both paths, because a citation to a deleted example is exactly
    # the drift a census exists to refuse.
    missing = check_assemblies(rows, lambda path: (ROOT / path).exists())
    if missing:
        for gone in missing:
            print(f"analyzer census: {gone}", file=sys.stderr)
        return 1
    # ★★★★★ R1807 — and the row that names NOTHING. Runs on every invocation,
    # beside `check_assemblies` rather than under `--check-proofs`, because it
    # builds nothing: it reads the pin. The expensive half (does the cited test
    # RUN?) stays behind the flag; whether a claim cited anything at all is
    # cheap enough to ask every time, and it is the half that was missing.
    unevidenced = check_evidence(rows)
    if unevidenced:
        for gap in unevidenced:
            print(f"analyzer census: {gap}", file=sys.stderr)
        return 1
    # ★★★★★ R1883 — and the verdict that earns the MOST by claiming the least.
    # `outside` takes a row out of the population entirely, and it was the only
    # verdict here with nothing to cite. Runs on every invocation beside the
    # three above: it reads the tracked store (0.11s) and builds nothing.
    try:
        sections = store_sections()
    except Finding as why:
        print(f"analyzer census: {why}", file=sys.stderr)
        return 1
    unbounded = check_boundaries(rows, sections)
    if unbounded:
        for claim in unbounded:
            print(f"analyzer census: {claim}", file=sys.stderr)
        return 1
    # ★★★★★ R1847 — and the substrate an `app` says is already there. Reads
    # files, builds nothing, so it runs every invocation beside the two above.
    adrift = check_rests(
        rows,
        lambda path: (ROOT / path).exists(),
        lambda path: (ROOT / path).read_text(encoding="utf-8", errors="replace"),
    )
    if adrift:
        for gone in adrift:
            print(f"analyzer census: {gone}", file=sys.stderr)
        return 1
    # The ratchet cannot be loosened by editing a row: a name in `UNASSEMBLED`
    # that no longer needs to be there is a stale exemption, and a stale
    # exemption is how a ratchet stops ratcheting.
    stale = sorted(
        ident
        for ident in UNASSEMBLED
        if not any(
            row["id"] == ident and row["verdict"] == "app" and not assembly_paths(row)
            for row in rows
        )
    )
    if stale:
        for ident in stale:
            print(
                f"analyzer census: {ident} is listed UNASSEMBLED and no longer "
                "needs to be — remove it from the ratchet",
                file=sys.stderr,
            )
        return 1
    if "--check-proofs" in sys.argv:
        # ★★★★★ R1771 — the proofs, against the test runner. Separate from
        # `--check-pin` because this one BUILDS: the pin's shape is cheap to
        # check on every push and this is not, and a check that made every push
        # wait for a test build is a check somebody would eventually route
        # around. It runs in the round that touches the census and in CI.
        try:
            listed = runner_tests(cited_crates(rows))
        except Finding as why:
            print(f"analyzer census: {why}", file=sys.stderr)
            return 1
        broken = check_proofs(rows, proof_oracle(listed), lambda p: (ROOT / p).exists())
        for gone in broken:
            print(f"analyzer census: {gone}", file=sys.stderr)
        cited = sum(len(proof_names(r)) + len(proof_paths(r)) for r in rows)
        print(
            f"analyzer census: {cited} proof citation(s) checked against "
            f"{len(listed)} selectable test(s), {len(broken)} unanswered"
        )
        return 1 if broken else 0
    if "--check-pin" in sys.argv:
        print(f"analyzer census: {len(rows)} capability(ies), pin well-formed")
        return 0
    print("\n".join(report(rows)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
