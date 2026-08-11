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
| `outside`    | not a framework concern at all (codecs, capture, supervision)   |
| `unmeasured` | **nobody has checked**, which is not the same as `have`         |

`unmeasured` is the point of the whole file. A row nobody has looked at is
invisible in a prose summary and is reported here, because the error direction
that matters is the silent one (R1602: a wrong `absent` self-corrects when the
next round reaches for it; a wrong `have` inflates a number nobody trips over).

## What this proves and what it does not

Completeness, only. Every declared capability carries a verdict, the verdicts
are from the closed set above, `have` rows name what covers them, and the counts
sum to the row count. Whether a `have` is TRUE is a separate question, and the
answer to it is the same as the reference census's: a test that exercises the
capability through the public API. `proven_by` is where that goes, and it is
empty on most rows today — which this reports rather than hides.

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
    python3 tools/analyzer_census.py --selftest  # the tool's own tests
    python3 tools/analyzer_census.py --check-pin # completeness only, for a hook
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PIN = ROOT / "docs" / "analyzer-census.json"

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
        if not row["id"].startswith(f"{row['plane']}."):
            raise Finding(f"{where}: an id is <plane>.<tier>.<n>")
    return rows


def assembly_paths(row: dict) -> list[str]:
    """The paths an `assembled_by` names.

    Pure, and deliberately loose about the prose around them: a token is a path
    if it contains a separator, so the field can read as a sentence and still be
    checkable. The alternative — a list field — makes the reason unwriteable,
    and a citation with no reason is what R1611 spent a round unwinding.
    """
    return [token.strip(",;") for token in str(row.get("assembled_by", "")).split() if "/" in token]


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
    if "--check-pin" in sys.argv:
        print(f"analyzer census: {len(rows)} capability(ies), pin well-formed")
        return 0
    print("\n".join(report(rows)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
