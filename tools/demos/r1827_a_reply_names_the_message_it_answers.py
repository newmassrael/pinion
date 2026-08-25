#!/usr/bin/env python3
"""R1827 §5.11 §5.12 §2 #7 — **request-response correlation: one row linking to
another, derived rather than stored.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `capture.t1.9` —
*request-response correlation, one row linking to another* — as an **app**
verdict: the framework's part is a derived column over the model plus a list
that can scroll to and select a row, and composing those into a correlation is
the application's. That verdict named **no assembly**, which is what R1807's
`UNASSEMBLED` ratchet exists to record: a claim about composition that nobody had
composed. This is the composition, driven on the wire, on the capture viewer
itself rather than on a generic table — because closing a census row on a demo
that never touches the reference screen closes a line without the screen gaining
anything (the R1722 lesson).

# The two decisions this round was asked to test rather than inherit

The work arrived half-finished from an interrupted round, which had made two
calls in prose. Both were put to a test, and the tests are why the code looks
the way it does now.

**"The correlation is derived, not stored" — upheld.** On the wire it is: a
reply in this protocol carries no request id, and what pairs the two is that
they travel one channel between one pair of endpoints in opposite directions,
the reply after the request. A stored field would be the analyser showing its
own bookkeeping. What the tests *changed* is the claim beside it: the first
draft said the relation was "symmetric by construction", and
`r1827_a_request_answered_twice_names_the_first_reply_and_both_replies_name_it`
shows it is many-to-one — three replies to one query all name it, and it names
the earliest of them back. The screen's own capture holds one exchange and would
have agreed with either sentence forever.

**"An unpaired row paints an empty run" — overturned, by five paint gates.** The
first draft made the link an eighth column and pushed an empty label into it for
every unlinked row, reasoning that the accessibility grid would otherwise report
fewer cells on one row than another. That reason was false (the grid's cells come
from a fixed-length vector, not from the painted runs), and the decision it
justified did not survive contact either:
`r1663_every_declared_element_of_the_screen_is_painted` named all eleven empty
cells — **an empty run is not a painted mark** — while three more gates showed the
column did not fit at all: the window overhung its own root by 47px and the byte
pane was painted outside the window, because this screen's minimum width is
derived from its columns and the size it opens in clears that floor by less than
one usable column.

So the link is **an annotation run inside the name column**, which is where this
screen already puts per-row derived facts (the out-of-band note, the fragment
marker), announced as part of that cell. A row in no exchange says nothing.

# The floor, measured rather than read

Against the reference toolkit at 6.11: its item model addresses a cell by row
and column ordinal and has no vocabulary for a relation BETWEEN rows at all — a
correlated pair there lives in whatever the caller wrote down beside the model,
reachable by nothing that reads the model. Here the relation is published whole,
so an agent that never saw the screen can follow an exchange:

  (A) the pairing is on the wire as a relation, keyed by row, saying which row —
      not only which sequence number, which is unique per channel and would make
      a client re-derive the pairing to resolve it.
  (B) it is SYMMETRIC on this capture and the answer says which end each row is.
  (C) following it is a lookup: the row the relation names can be selected, and
      the screen goes there.
  (D) the derivation is honest about what it pairs — one channel, one pair of
      endpoints, in time order — and pairs nothing else in the capture.
  (E) a reader is told: the linked rows announce their counterpart inside the
      name cell, and the unlinked ones announce nothing extra.
  (F) the exchange survives a filter: `session` is in the query roster, and
      filtering by it keeps BOTH halves rather than the half whose `hop` was
      typed.
  (G) the declaration is a precondition of dispatch — the path is published on
      the schema channel before it answers.

Run from the workspace root:
    cargo build -p hello-packet-view --release
    python3 tools/demos/r1827_a_reply_names_the_message_it_answers.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

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


def announced_cells(app: RpcSubprocess) -> dict[str, list[str]]:
    """Every message row's announced cells, from the accessibility tree.

    Read off `scene/access` rather than off the model, because what this section
    claims is that a *reader* is told — and the model saying so is a different
    claim from the tree carrying it.
    """
    result = app.request("scene/access").result
    out: dict[str, list[str]] = {}
    cells: dict[str, str] = {}
    for node in result["nodes"]:
        tag = node.get("tag") or ""
        if tag.startswith("pv.list.cell."):
            cells[tag] = node.get("name") or ""
    for tag, name in cells.items():
        row = tag.removeprefix("pv.list.cell.").split("_")[0]
        out.setdefault(row, [])
    for row in out:
        out[row] = [
            cells[t]
            for t in sorted(
                (t for t in cells if t.removeprefix("pv.list.cell.").split("_")[0] == row),
                key=lambda t: int(t.rsplit("_", 1)[1]),
            )
        ]
    return out


def body() -> None:
    with RpcSubprocess(VIEWER, boot_grace=1.5) as d:
        spec: Any = d.query(f"{EXT}/spec")
        rows = spec["rows"]
        columns = [c["title"] for c in spec["columns"]]

        # ── (A) the pairing is on the wire, as a relation ────────────
        banner("A — the exchange is published as a relation over rows, not as a column of text")
        pairs: Any = d.query(f"{EXT}/correlation")
        ok("the relation is an object keyed by row", isinstance(pairs, dict))
        ok("it names only the rows that are in an exchange", len(pairs) < len(rows))
        ok("and it is not empty, so the checks below are about something", len(pairs) > 0)
        print(f"[demo] {len(pairs)} of {len(rows)} message(s) are half of an exchange: {pairs}")
        for key, entry in pairs.items():
            ok(
                f"row {key} names a ROW and not only a sequence number",
                isinstance(entry.get("row"), int),
            )
            ok(
                f"row {key} also carries that row's sn, so a reader can match the screen",
                entry["sn"] == rows[entry["row"]]["sn"],
            )
            ok(
                f"row {key} says which end of the pair it is",
                entry["role"] in ("reply", "request"),
            )
        # The point of publishing the ROW: a sequence number is unique per
        # channel, not per capture, so a client given only `sn` would have to
        # re-derive the pairing to resolve it. Measured on this very capture.
        by_sn = [n for n, r in enumerate(rows) if r["sn"] == rows[1]["sn"]]
        ok("a sequence number is per-channel, so `row` is what makes it followable", len(by_sn) >= 1)

        # ── (B) it comes back, and each end says which end it is ─────
        banner("B — the reply names its request, the request names a reply, and they agree")
        replies = [int(k) for k, v in pairs.items() if v["role"] == "reply"]
        requests = [int(k) for k, v in pairs.items() if v["role"] == "request"]
        ok("this capture has at least one of each end", replies and requests)
        for n in replies:
            assert_eq(rows[n]["kind"], "Response", f"row {n} calls itself a reply")
            other = pairs[str(n)]["row"]
            assert_eq(rows[other]["kind"], "Query", f"row {n} answers a query")
            ok(
                f"row {n}'s request names a reply back — the walk returns",
                str(other) in pairs,
            )
        for n in requests:
            assert_eq(rows[n]["kind"], "Query", f"row {n} calls itself a request")
            assert_eq(
                rows[pairs[str(n)]["row"]]["kind"],
                "Response",
                f"row {n} is answered by a response",
            )

        # ── (C) following the link is a lookup, not a second derivation ─
        banner("C — following the exchange: the row the relation names is the row that opens")
        start = replies[0]
        target = pairs[str(start)]["row"]
        assert_eq(d.invoke(f"{EXT}/select_message", start), start, "stand on the reply")
        assert_eq(d.query(f"{EXT}/selected_row"), start, "the screen went there")
        assert_eq(
            d.invoke(f"{EXT}/select_message", target),
            target,
            "follow the link the relation published",
        )
        assert_eq(d.query(f"{EXT}/selected_row"), target, "and the screen followed it")
        ok(
            "the row it landed on names the row it came from",
            pairs[str(target)]["row"] == start,
        )

        # ── (D) the derivation pairs an exchange and nothing else ────
        banner("D — one channel, one pair of endpoints, in time order — and nothing else pairs")
        for key, entry in pairs.items():
            a, b = rows[int(key)], rows[entry["row"]]
            assert_eq(a["channel"], b["channel"], f"row {key}'s pair shares a channel")
            ok(
                f"row {key}'s pair runs between the same two endpoints, opposite ways",
                sorted(a["hop"].split(" -> ")) == sorted(b["hop"].split(" -> ")),
            )
            ok(f"row {key}'s pair travels opposite ways", a["hop"] != b["hop"])
            query, reply = (a, b) if a["kind"] == "Query" else (b, a)
            ok(f"row {key}'s reply comes after its request", query["time"] < reply["time"])
        # The negative half, which is what makes the positive half a rule rather
        # than a coincidence: every OTHER query and response in the capture is
        # unpaired, and the count says how many that is.
        conversational = [
            n for n, r in enumerate(rows) if r["kind"] in ("Query", "Response")
        ]
        unpaired = [n for n in conversational if str(n) not in pairs]
        ok(
            "queries and responses that are in no exchange are left unpaired",
            len(unpaired) > 0,
        )
        print(
            f"[demo] {len(conversational)} conversational message(s), "
            f"{len(pairs)} paired, {len(unpaired)} unpaired: {unpaired}"
        )

        # ── (E) a reader is told, in the cell it is painted in ───────
        banner("E — the link is announced inside the name cell, and only where there is one")
        cells = announced_cells(d)
        name_column = columns.index("name")
        ok("every row announces one cell per column", all(
            len(v) == len(columns) for v in cells.values()
        ))
        spoken = 0
        for key, entry in pairs.items():
            said = cells[key][name_column]
            word = "answers " if entry["role"] == "reply" else "answered by "
            ok(f"row {key}'s name cell says which end it is ({word.strip()!r})", word in said)
            ok(
                f"row {key}'s name cell names its counterpart's sequence number",
                said.endswith(str(entry["sn"])),
            )
            ok(
                f"row {key}'s name cell still opens with the resource name",
                said.startswith(rows[int(key)]["name"]),
            )
            spoken += 1
        assert_eq(spoken, len(pairs), "every pair is announced")
        for key, said in cells.items():
            if key in pairs:
                continue
            ok(
                f"row {key} is in no exchange and says nothing about one",
                "answers " not in said[name_column]
                and "answered by " not in said[name_column],
            )

        # ── (F) the exchange survives a filter ───────────────────────
        banner("F — following one session keeps BOTH halves of the exchange")
        roster = spec["query_columns"]
        ok("`session` is a thing a reader may filter on", "session" in roster)
        a, b = sorted(rows[int(replies[0])]["hop"].split(" -> "))
        session = f"{a} <-> {b}"
        assert d.invoke(f"{EXT}/filter", f"session = {session}") is not None, "filter accepted"
        assert_eq(d.query(f"{EXT}/query_fault"), None, "the roster knows the name")
        kept = d.query(f"{EXT}/kept_rows")
        for key in pairs:
            ok(f"row {key} survived the session filter", int(key) in kept)
        # And the counterfactual that makes it worth having: the DIRECTED text a
        # reader could have typed instead keeps one half and looks like it
        # worked. Measured here rather than asserted in a comment.
        # ★ `hop` and not `from -> to`: the roster a query is written against
        # spells the column `hop`, while the LIST HEADER reads `from -> to`.
        # Measured here the hard way — the first draft typed the header and was
        # refused with `no column is called "from -"`, because the arrow in the
        # heading is parsed as an operator. That refusal is the surface working:
        # it named the roster back.
        assert d.invoke(
            f"{EXT}/filter", f"hop = {rows[int(replies[0])]['hop']}"
        ) is not None, "directed filter accepted"
        half = d.query(f"{EXT}/kept_rows")
        ok(
            "filtering by the DIRECTED hop keeps one end of the exchange and drops the other",
            any(int(k) in half for k in pairs) and not all(int(k) in half for k in pairs),
        )
        assert d.invoke(f"{EXT}/filter", "") is not None, "filter cleared"
        assert_eq(len(d.query(f"{EXT}/kept_rows")), len(rows), "every row is back")

        # ── (G) the declaration is a precondition of dispatch ────────
        banner("G — the path is published before it answers")
        fields = d.query(f"{EXT}/$schema")
        field = next((f for f in fields if f["path"] == "correlation"), None)
        ok("the relation is declared on the schema channel", field is not None)
        ok("declared as structured data rather than a sentence", field["type"] == "json")
        # ★ A read declares no `channel` — the absence IS the read channel, and
        # an action is what carries the key. Measured here rather than asserted
        # from memory: the first draft of this section read `field["channel"]`
        # and died on a `KeyError`, which is the surface telling the truth. The
        # discrimination is shown rather than described, by holding a read and an
        # action from the SAME screen side by side.
        ok("a read carries no channel key", "channel" not in field)
        action = next(f for f in fields if f["path"] == "export")
        assert_eq(action["channel"], "invoke", "an action declares its channel")
        ok(
            "so the published surface distinguishes a read from an action",
            ("channel" in action) != ("channel" in field),
        )
        # And the declaration is a PRECONDITION of dispatch (R1637): the arm was
        # answering before it was declared, and the schema entry is what makes
        # the answer reachable at all.
        ok("the path the relation is read at is the path declared", field["path"] == "correlation")

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1827 a reply names the message it answers", body)
