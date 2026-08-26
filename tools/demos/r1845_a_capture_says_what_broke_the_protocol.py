#!/usr/bin/env python3
"""R1845 §5.11 §5.12 §5.40 §2 #7 — **the capture says which of its rows broke
the protocol, and a client can check every verdict against the rows.**

# What this demo exists for

The analysis-tool census carries `capture.t2.18` — *protocol violation rows:
sequence regression, undeclared reference, unnegotiated extension*. Every
ingredient had been in `spec::RowSpec` since the screen was written: a
per-channel sequence number, a name that doubles as the declaration a row
establishes, a note that carries an extension marker. **Nothing asked.**

So the row was not blocked on data, it was blocked on a *reading* — and the
reading could not be built until the capture it reads was coherent, which is the
debt this round opened and paid: the rows were written newest-first while every
channel's sequence numbers ascended downward, so a detector over them would have
reported ten regressions that were facts about the TABLE rather than about a
protocol.

# The two things that had to be worked out, and both are visible below

**(1) A watermark, not an adjacent difference.** Differencing neighbouring pairs
reports a break on BOTH SIDES of a row that goes backwards, because a backwards
row makes each of its neighbours look non-consecutive. On this capture's own
`data/rel` that reading manufactures *two* breaks where *nothing* is missing.
Section F shows the two readings disagreeing on the live rows, so the choice is
demonstrated rather than described.

**(2) A declaration and a reference are spelled the same way.** `id N -> path`
is the text of both; which one it is comes from the row's KIND. And the check is
made over the WHOLE capture rather than "declared before it was used" — a
declaration this capture never contains is a different, stronger fact than one
that arrives late, and the weaker one needs an ordering claim the reference does
not make.

# What is shown

  (A) the two reads are DECLARED before either answers — the fact and its
      vocabulary, on the schema channel.
  (B) the vocabulary is CLOSED and PUBLISHED: a client enumerates what can be
      reported instead of discovering the words from a sample that happens to
      contain them. Every kind occurs in this capture, so no clause is vacuous.
  (C) every verdict addresses a real row and states a reason.
  (D) ★ the verdicts are DERIVED, not stored: the whole table is recomputed
      here, from the rows and the negotiated set alone, and comes out equal.
  (E) the reassembly strip is the SAME reading — a lane and a violation row
      cannot disagree about whether a channel went backwards — and the strip's
      carrying count is the CAPTURE's, not the lane roster's.
  (F) the watermark and adjacent differencing give different answers here.

Run from the workspace root:
    cargo build -p hello-packet-view --release
    python3 tools/demos/r1845_a_capture_says_what_broke_the_protocol.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

VIEWER = "hello-packet-view"
EXT = "/external"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def named_id(name: str) -> Optional[int]:
    """The id a row's name establishes or uses, when it names one.

    Deliberately answers only the NUMBER: a declaration and a reference are
    spelled identically and it is the row's kind that tells them apart, so a
    helper that judged here would be judging without the fact it needs.
    """
    if not name.startswith("id "):
        return None
    head = name[len("id ") :].split()
    if not head or not head[0].isdigit():
        return None
    return int(head[0])


def channels(rows: list[dict[str, Any]]) -> list[str]:
    """Every channel the capture carries, in the order it first shows one."""
    out: list[str] = []
    for row in rows:
        if row["channel"] not in out:
            out.append(row["channel"])
    return out


def series(rows: list[dict[str, Any]], channel: str) -> list[tuple[int, int]]:
    """One channel's rows and sequence numbers, OLDEST FIRST.

    The capture is newest-first, so this walks it backwards. A sequence is a
    claim about time and the table is ordered for a reader.
    """
    return [
        (n, row["sn"])
        for n in range(len(rows) - 1, -1, -1)
        for row in [rows[n]]
        if row["channel"] == channel
    ]


def regressions(rows: list[dict[str, Any]], channel: str) -> list[int]:
    """The rows on one channel that fail to beat the number it had reached."""
    highest: Optional[int] = None
    out: list[int] = []
    for n, sn in series(rows, channel):
        if highest is not None and sn <= highest:
            out.append(n)
        highest = sn if highest is None else max(highest, sn)
    return out


def skipped(rows: list[dict[str, Any]], channel: str) -> list[int]:
    """The numbers between a channel's lowest and its highest that never came."""
    seen = {sn for _, sn in series(rows, channel)}
    if not seen:
        return []
    return [n for n in range(min(seen), max(seen)) if n not in seen]


def derive(rows: list[dict[str, Any]], negotiated: list[str]) -> list[tuple[str, int]]:
    """The whole violation table, recomputed from the capture alone.

    This is section D's instrument: if the screen were storing its verdicts
    beside the rows rather than reading them, this would drift the first time
    either half was edited.
    """
    found: list[tuple[str, int]] = []
    for channel in channels(rows):
        for n in regressions(rows, channel):
            found.append(("sequence regression", n))
    declared = {
        named_id(row["name"]) for row in rows if row["kind"] == "Declare"
    } - {None}
    for n, row in enumerate(rows):
        if row["kind"] == "Declare":
            continue
        used = named_id(row["name"])
        if used is not None and used not in declared:
            found.append(("undeclared reference", n))
    for n, row in enumerate(rows):
        note = row.get("note") or ""
        if note.startswith("extension "):
            marker = note[len("extension ") :].strip()
            if marker not in negotiated:
                found.append(("unnegotiated extension", n))
    found.sort(key=lambda pair: pair[1])
    return found


def counts_sentence(app: RpcSubprocess) -> str:
    """What the strip's totals node announces, from the accessibility tree."""
    for node in app.request("scene/access").result["nodes"]:
        if node.get("tag") == "pv.reassembly.counts":
            return json.dumps(node, ensure_ascii=False)
    raise AssertionError("the reassembly strip announces no totals")


def body() -> None:
    with RpcSubprocess(VIEWER, boot_grace=1.5) as d:
        spec: Any = d.query(f"{EXT}/spec")
        rows = spec["rows"]
        lanes = spec["lanes"]
        negotiated = spec["negotiated_extensions"]

        # ── (A) declared before either answers ───────────────────────
        banner("A — the fact and its vocabulary are published on the schema channel")
        fields = d.query(f"{EXT}/$schema")
        found = {f["path"]: f for f in fields}
        ok("the violations are declared as a read slot", "violations" in found)
        ok("so is the vocabulary they are drawn from", "violation_kinds" in found)
        assert_eq(found["violations"]["type"], "json", "the table declares its wire form")
        assert_eq(
            found["violation_kinds"]["type"],
            "string",
            "the vocabulary declares its wire form",
        )
        for path in ("violations", "violation_kinds"):
            ok(
                f"{path} is a read and carries no channel key",
                "channel" not in found[path],
            )

        # ── (B) a closed, published vocabulary ───────────────────────
        banner("B — the vocabulary is CLOSED, and every word of it is in this capture")
        kinds = d.query(f"{EXT}/violation_kinds").split(",")
        ok("the screen publishes more than one kind", len(kinds) > 1)
        ok("and no word is repeated", len(set(kinds)) == len(kinds))
        violations = d.query(f"{EXT}/violations")
        ok("the capture reports violations", len(violations) > 0)
        for v in violations:
            ok(f"{v['kind']!r} is a word the screen published", v["kind"] in kinds)
        # ★ A detector whose capture contains none of what it detects asserts
        # nothing — every clause passes over an empty set. So each published word
        # has to be reachable, and this capture carries one of each on purpose.
        for kind in kinds:
            hits = [v for v in violations if v["kind"] == kind]
            ok(f"{kind!r} is exercised by this capture", len(hits) == 1)
        print(f"[demo] vocabulary: {kinds}")

        # ── (C) every verdict is addressed and reasoned ──────────────
        banner("C — a verdict points at a row and says why")
        for v in violations:
            ok(f"row {v['row']} is a row this capture has", 0 <= v["row"] < len(rows))
            ok(f"row {v['row']} is given a reason", len(v["why"]) > 20)
            row = rows[v["row"]]
            print(
                f"[demo] {v['kind']:<22} row {v['row']:>2}  "
                f"{row['time']} {row['channel']:<10} sn={row['sn']:<5} "
                f"{row['kind']:<9} {row['name']}"
            )
            print(f"[demo] {'':<22}         -> {v['why']}")

        # ── (D) DERIVED, not stored ──────────────────────────────────
        banner("D — the whole table is recomputed from the rows, and comes out equal")
        mine = derive(rows, negotiated)
        assert_eq(
            [(v["kind"], v["row"]) for v in violations],
            mine,
            "the screen's verdicts are not what the capture's own rows say",
        )
        # Each of the three, checked against its own premise rather than against
        # the recomputation as a whole — a single equality can be satisfied by
        # two identical mistakes.
        reference = next(v for v in violations if v["kind"] == "undeclared reference")
        used = named_id(rows[reference["row"]]["name"])
        ok("the flagged reference names an id", used is not None)
        ok(
            "and no row of this capture declares it",
            all(
                named_id(row["name"]) != used
                for row in rows
                if row["kind"] == "Declare"
            ),
        )
        ok(
            "while the ids that ARE declared are not flagged",
            any(named_id(row["name"]) is not None for row in rows if row["kind"] == "Declare"),
        )
        extension = next(v for v in violations if v["kind"] == "unnegotiated extension")
        marker = rows[extension["row"]]["note"][len("extension ") :].strip()
        ok("the session published what it negotiated", len(negotiated) > 0)
        ok(f"and {marker} is not in it", marker not in negotiated)
        print(f"[demo] negotiated {negotiated}, and the capture carries {marker}")

        # ── (E) the strip is the same reading ────────────────────────
        banner("E — a lane and a violation row cannot disagree about a channel")
        for lane in lanes:
            channel = lane["channel"]
            assert_eq(
                lane["sn"],
                max(sn for _, sn in series(rows, channel)),
                f"{lane['name']} does not show the number {channel} reached",
            )
            assert_eq(
                lane["out_of_order"],
                len(regressions(rows, channel)),
                f"{lane['name']} disagrees with the rows about going backwards",
            )
            assert_eq(
                lane["skipped"],
                skipped(rows, channel),
                f"{lane['name']} disagrees with the rows about what is missing",
            )
            assert_eq(
                lane["continuous"],
                not lane["skipped"] and lane["out_of_order"] == 0,
                f"{lane['name']} says the wrong thing about its continuity",
            )
            print(
                f"[demo] lane {lane['name']:<28} sn={lane['sn']:<5} "
                f"continuous={lane['continuous']} skipped={lane['skipped']} "
                f"out_of_order={lane['out_of_order']} dropped={lane['dropped']}"
            )
        # ★★★★★ THE TWO TERMS OF CONTINUITY ARE SEPARATELY EXERCISED, and this
        # pair of checks exists because a counterfactual found that they were
        # not: while every lane with a row out of order also had a number
        # missing, a continuity that ignored the out-of-order half was
        # indistinguishable from the right one and nothing failed. The capture
        # was renumbered to put the two faults on different channels, and these
        # are what refuse a later edit that puts them back together.
        ok(
            "one lane is broken ONLY by arriving out of order",
            any(not x["continuous"] and not x["skipped"] for x in lanes),
        )
        ok(
            "and one ONLY by a number that never arrived",
            any(not x["continuous"] and x["out_of_order"] == 0 for x in lanes),
        )
        regressing = {rows[v["row"]]["channel"] for v in violations if v["kind"] == kinds[0]}
        ok("the reported regression is on a channel the strip draws", regressing)
        for channel in regressing:
            lane = next(x for x in lanes if x["channel"] == channel)
            ok(
                f"and {lane['name']} does not call itself unbroken",
                not lane["continuous"],
            )
        # ★ THE HEADER COUNTS THE CAPTURE, NOT THE ROSTER. Two different facts,
        # and this capture is what makes them distinguishable: it carries a
        # channel the strip draws no lane for.
        carrying = spec["channels_carrying"]
        assert_eq(carrying, channels(rows), "the strip's carrying list is the capture's")
        ok(
            "the capture carries a channel the strip leaves undrawn",
            len(carrying) > len(lanes),
        )
        said = counts_sentence(d)
        ok(
            "the totals announce the CAPTURE's count",
            f"{len(carrying)} of " in said,
        )
        ok(
            "and not the lane roster's",
            f"{len(lanes)} of " not in said,
        )

        # ── (F) the watermark, shown against the naive reading ───────
        banner("F — a regression does not manufacture gaps, and here it would have")
        for channel in channels(rows):
            walk = [sn for _, sn in series(rows, channel)]
            naive = sum(1 for a, b in zip(walk, walk[1:]) if b > a + 1)
            if naive != len(skipped(rows, channel)):
                print(
                    f"[demo] {channel}: adjacent differencing says {naive} break(s), "
                    f"the watermark says {len(skipped(rows, channel))} missing "
                    f"+ {len(regressions(rows, channel))} out of order — {walk}"
                )
                ok(
                    f"the two readings disagree on {channel}, so the choice is not free",
                    True,
                )
                break
        else:
            raise AssertionError(
                "no channel distinguishes the two readings, so section F asserts nothing"
            )

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1845 a capture says what broke the protocol", body)
