#!/usr/bin/env python3
"""R1829 §5.11 §5.12 §5.40 — **follow one session, in time order.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `capture.t1.11` —
*follow one session, in time order* — as an **app** verdict whose covering
sentence is *a filter plus a sort over the substrate above*. That verdict named
**no assembly**, which is what R1807's `UNASSEMBLED` ratchet records: a claim
about composition that nobody had composed. This is the composition, driven on
the wire, on the capture viewer itself rather than on a generic table — closing
a census row on a demo that never touches the reference screen closes a line
without the screen gaining anything (the R1722 lesson).

# The premise, measured before anything was built

**This capture is written NEWEST FIRST**, which is what makes the second half of
the capability a capability rather than a decoration: filtering to one session
gives you both halves of an exchange with the REPLY ABOVE THE REQUEST IT
ANSWERS. A reader following the conversation top to bottom meets the answer
before the question. Section B measures that on the running screen rather than
asserting it, because every later section would pass against a screen that
cannot sort at all if the capture happened to be chronological already.

# What was composed, and what was deliberately not

The ordering is `pinion_core::widgets::table::grid_order_by` — the framework's
ordering SSOT, the one the virtualised data grid runs on — with `cell_cmp` as
the comparison, `cycle_col_sort` as the header transition, `grid_sort_str` /
`grid_sort_parse` as the wire vocabulary, and `col_sort_dir` +
`SortDirection::from_ascending` for `aria-sort`. Nothing here spells its own.

What was NOT taken is `GridSortState`, and the reason is a defect this tree
already carries twice: that type owns the cells, the filter AND the sort, while
this screen's filter is `RowQuery` — which parses column NAMES and keeps each
clause's source text, strictly richer than the `GridFilter` it carries. Adopting
the whole object would have put two filter models on one list. The ordering is
published separately from the state that holds it, which is exactly what makes
taking only the half that is wanted possible.

# The trap this feature sits on

`cell_cmp` chooses its comparison by trying to `parse::<f64>()` BOTH cells: it
sorts numerically when both parse and lexically otherwise. So `len` and `sn`
sort as numbers while `time` sorts as text — and text is chronological here only
because every timestamp is fixed-width `HH:MM:SS.mmm`, which R1827 pinned with a
test for a different reason and this feature now depends on. A comparator picked
by a `parse` is one a reader cannot see at the call site, so section D asserts
both branches and section D's counterfactual shows the two disagree on this very
capture.

# What is shown

  (A) the order is published in the framework's own wire vocabulary, read at
      the slot `sort` and written at the verb `order` — a slot names the fact
      and a verb names the act, which is the rule this screen already runs on
      one axis over (`query` / `filter`), and which a single path carrying both
      would have broken: the wire refuses that with `PathIsAReadSlot`.
  (B) the capture opens newest-first: the reply sits ABOVE its request.
  (C) following one session keeps BOTH halves and drops the rest.
  (D) ordering by time turns the kept run into the conversation — request then
      reply — and the whole run is chronological, while ordering by length is
      NUMERIC, which is a different answer on this capture.
  (E) the reader is told: exactly one column carries `aria-sort`, it is the
      ordered one, and it says which way.
  (F) the POINTER does it too: pressing the column header cycles
      unsorted -> ascending -> descending -> unsorted.
  (G) an order this screen cannot read is refused, and the two ways to get it
      wrong are told apart by name.
  (H) the declaration is a precondition of dispatch — the read and the action
      are published on the schema channel before either answers.

Run from the workspace root:
    cargo build -p hello-packet-view --release
    python3 tools/demos/r1829_following_one_session_reads_it_in_time_order.py
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


def head_sort(app: RpcSubprocess) -> dict[int, str]:
    """Which column headers carry `aria-sort`, from the accessibility tree.

    Read off `scene/access` and not off the model, because what section E claims
    is that a *reader* is told, and the model saying so is a different claim
    from the tree carrying it.
    """
    out: dict[int, str] = {}
    for node in app.request("scene/access").result["nodes"]:
        tag = node.get("tag") or ""
        if tag.startswith("pv.list.head.") and node.get("sort"):
            out[int(tag.removeprefix("pv.list.head."))] = node["sort"]
    return out


def body() -> None:
    with RpcSubprocess(VIEWER, boot_grace=1.5) as d:
        spec: Any = d.query(f"{EXT}/spec")
        rows = spec["rows"]
        columns = [c["title"] for c in spec["columns"]]
        time_col = columns.index("time")
        len_col = columns.index("len")

        # ── (A) the order is published, in the framework's vocabulary ─
        banner("A — the list's order is on the wire, and read and write share one vocabulary")
        assert_eq(d.query(f"{EXT}/sort"), "none", "the screen opens in capture order")
        # ★ A SLOT NAMES THE FACT AND A VERB NAMES THE ACT — read at `sort`,
        # written at `order`, one value vocabulary between them. That is this
        # screen's own rule, already running one axis over as `query` / `filter`,
        # and it is what a path carrying both would have broken: the wire refuses
        # a path declared as a read AND an action with `PathIsAReadSlot`.
        for spelling in ["0:ascending", "3:descending", "none"]:
            d.invoke(f"{EXT}/order", spelling)
            assert_eq(d.query(f"{EXT}/sort"), spelling, f"{spelling} reads back as written")
        ok(
            "an order read off the slot can be handed straight back to the verb",
            True,
        )

        # ── (B) the premise: this capture is newest-first ────────────
        banner("B — the capture opens NEWEST FIRST, so a reply sits above its request")
        pairs: Any = d.query(f"{EXT}/correlation")
        ok("this capture holds an exchange to follow", len(pairs) >= 2)
        reply = next(int(k) for k, v in pairs.items() if v["role"] == "reply")
        request = pairs[str(reply)]["row"]
        ok(
            "the reply is ABOVE the request it answers, in capture order",
            reply < request,
        )
        ok(
            "and it is later in TIME, which is what makes that an ordering problem",
            rows[request]["time"] < rows[reply]["time"],
        )
        print(
            f"[demo] row {request} at {rows[request]['time']} is answered by "
            f"row {reply} at {rows[reply]['time']} — the answer is {request - reply} "
            f"row(s) higher up the list"
        )

        # ── (C) following one session keeps both halves ──────────────
        banner("C — following one session keeps BOTH halves of the exchange")
        a, b = sorted(rows[reply]["hop"].split(" -> "))
        session = f"{a} <-> {b}"
        assert d.invoke(f"{EXT}/filter", f"session = {session}") is not None, "filter accepted"
        assert_eq(d.query(f"{EXT}/query_fault"), None, "the roster knows `session`")
        kept = d.query(f"{EXT}/kept_rows")
        ok("the request survived", request in kept)
        ok("the reply survived", reply in kept)
        ok("and the rest of the capture did not", len(kept) < len(rows))
        ok(
            "in capture order the answer still comes first — nothing is fixed yet",
            kept.index(reply) < kept.index(request),
        )
        print(f"[demo] following {session!r} keeps {len(kept)} of {len(rows)}: {kept}")

        # ── (D) ordering by time makes it the conversation ───────────
        banner("D — ordering by time turns the kept run into the conversation")
        answer = d.invoke(f"{EXT}/order", f"{time_col}:ascending")
        walked = d.query(f"{EXT}/kept_rows")
        ok("ordering kept exactly the rows the filter kept", sorted(walked) == sorted(kept))
        ok(
            "the request now comes BEFORE the reply it was answered by",
            walked.index(request) < walked.index(reply),
        )
        times = [rows[n]["time"] for n in walked]
        ok("and the whole kept run is chronological", times == sorted(times))
        ok(
            "the action answers with the row now at the top, not a bare acknowledgement",
            answer.get("top") == walked[0],
        )
        print(f"[demo] ordered by time: {walked} -> {times}")
        # ★ The comparator trap, shown rather than described. `cell_cmp` picks
        # numeric-vs-lexical by trying to parse BOTH cells as f64, so `len`
        # sorts as a NUMBER while `time` sorts as text. On this capture the two
        # give different answers, which is what makes the check able to fail.
        assert d.invoke(f"{EXT}/filter", "") is not None, "filter cleared"
        d.invoke(f"{EXT}/order", f"{len_col}:ascending")
        lens = [rows[n]["len"] for n in d.query(f"{EXT}/kept_rows")]
        ok("length orders NUMERICALLY", lens == sorted(lens))
        ok(
            "and a lexical order of the same lengths differs, so that check can fail",
            [str(x) for x in lens] != sorted(str(x) for x in lens),
        )
        print(f"[demo] ordered by len: {lens}")

        # ── (E) the reader is told which column orders the list ──────
        banner("E — exactly one column carries aria-sort, and it says which way")
        d.invoke(f"{EXT}/order", "none")
        assert_eq(head_sort(d), {}, "an unsorted list announces no sorted column")
        d.invoke(f"{EXT}/order", f"{time_col}:ascending")
        assert_eq(
            head_sort(d),
            {time_col: "ascending"},
            "the ordered column is the one that announces it",
        )
        d.invoke(f"{EXT}/order", f"{time_col}:descending")
        assert_eq(head_sort(d), {time_col: "descending"}, "and the direction is announced")

        # ── (F) the pointer does it too ──────────────────────────────
        banner("F — pressing the column header cycles the order")
        d.invoke(f"{EXT}/order", "none")
        head = f"pv.list.head.{time_col}"
        seen = []
        for _ in range(3):
            d.click(path=head)
            seen.append(d.query(f"{EXT}/sort"))
        assert_eq(
            seen,
            [f"{time_col}:ascending", f"{time_col}:descending", "none"],
            "a header press cycles unsorted -> ascending -> descending -> unsorted",
        )
        d.click(path=head)
        d.click(path=f"pv.list.head.{len_col}")
        assert_eq(
            d.query(f"{EXT}/sort"),
            f"{len_col}:ascending",
            "pressing a different column jumps straight to it ascending",
        )
        d.invoke(f"{EXT}/order", "none")

        # ── (G) a refusal says which way it was wrong ────────────────
        banner("G — an order this screen cannot read is refused, by name")
        gibberish = None
        try:
            d.invoke(f"{EXT}/order", "by time please")
        except Exception as why:  # noqa: BLE001 — the refusal is the subject
            gibberish = str(why)
        ok("a string that is not an order is refused", gibberish is not None)
        ok("and the refusal says so", "is not an order" in (gibberish or ""))
        phantom = None
        try:
            d.invoke(f"{EXT}/order", "99:ascending")
        except Exception as why:  # noqa: BLE001
            phantom = str(why)
        ok("a column that does not exist is refused", phantom is not None)
        ok("and the refusal names the column", "no column 99" in (phantom or ""))
        ok(
            "the two are told apart — a caller who mistyped is not sent column-hunting",
            gibberish != phantom,
        )
        assert_eq(d.query(f"{EXT}/sort"), "none", "a refused order left the list alone")

        # ── (H) the declaration is a precondition of dispatch ────────
        banner("H — the read and the action are published before either answers")
        fields = d.query(f"{EXT}/$schema")
        read = next((f for f in fields if f["path"] == "sort"), None)
        action = next((f for f in fields if f["path"] == "order"), None)
        ok("the fact is declared as a read slot", read is not None)
        ok("the act is declared as an action", action is not None)
        ok("a read carries no channel key", "channel" not in read)
        assert_eq(action["channel"], "invoke", "the action declares its channel")
        assert_eq(read["type"], "string", "the read is declared as the wire form it answers")
        assert_eq(action["type"], "string", "and the action takes that same form")
        ok(
            "the two carry DIFFERENT paths — which is what the wire requires",
            read["path"] != action["path"],
        )

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1829 following one session reads it in time order", body)
