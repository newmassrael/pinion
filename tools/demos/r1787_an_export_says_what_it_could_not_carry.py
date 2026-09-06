#!/usr/bin/env python3
"""R1787 §5.38 §5.15 §2 #2 §2 #7 — **exporting a range, and naming every cell the
chosen dialect could not carry.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries
`capture.t1.12` — *marking, bookmarks and comments, plus exporting a range* — as
a **gap**, on the recorded reason "no tabular/CSV export derivation exists in any
crate".

Re-measured before this round wrote a line of code, that reason was wrong in
both directions. A derivation existed (`widgets::table::rows_to_tsv`, R1372) and
what it lacked was not existence: it is *structure-preserving over
content-faithful* by design — a cell holding the delimiter is rewritten with a
space so the block's shape survives — and it made that trade **silently**. Its
own doc deferred the rest ("full spreadsheet-style quoting is a later
enhancement"). So the real gap was faithfulness, and a report.

# The floor, measured rather than read

Built and run against the reference toolkit at 6.11 (scratchpad only, never
tracked):

* asked for a rectangle of four cells *as data*, its item-model layer answers
  **two binary payloads**, `hasText` false, text length `0`, and neither the
  header labels nor the cell text recoverable from the bytes;
* its tabular widget, every cell selected and a real copy chord delivered,
  leaves the clipboard holding **no format at all** — `formats` is null,
  `text()` is empty.

So the floor for "export this range as text" is nothing, and the bar is set by
what the capture itself contains rather than by a competitor's surface.

# Why this screen, and why the losses are not hypothetical

`capture.t1.12` is the **capture viewer's** row, so closing it on a generic
table demo would close a census line without the reference screen gaining
anything (R1722's lesson). It is driven here on screen B itself. And screen B's
own data forces the point: **one cell of the message list holds a comma**
(`… Last 3/3 reassembled 3,144 B`), so a naive comma-separated export of this
very screen splits a column silently.

⚠ That number is **one**, and the first draft of this file said seven. Seven is
what a grep of the fixture answers — six of those literals belong to the decode
tree, which the message list does not export. The count here is what the wire
returned. Population is a thing to measure, not to read off a file.

  (A) the dialect roster is readable BEFORE exporting, and says which dialects
      can carry any cell unchanged.
  (B) a faithful dialect exports the whole capture, reports NO loss, and the
      block reads back cell-for-cell — including the comma-bearing ones.
  (C) the header line is there, and it is the screen's own column titles.
  (D) the lossy clipboard dialect keeps the block's SHAPE and names the cells it
      flattened, by line and column, with what was in them.
  (E) `scope` follows the filter: exporting `shown` after a query carries the
      kept rows and only those, and `rows` says which.
  (F) every way of getting the call wrong answers a sentence naming what was
      wrong, not a tag.
  (G) the declaration is a precondition of dispatch: `export` publishes its
      argument grammar, and each argument's domain says where its values come
      from.

Run from the workspace root:
    cargo build -p hello-packet-view --release
    python3 tools/demos/r1787_an_export_says_what_it_could_not_carry.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
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


def export(d: RpcSubprocess, dialect: str, scope: str) -> dict:
    return d.invoke(f"{EXT}/export", {"dialect": dialect, "scope": scope})


def read_back(text: str) -> list[list[str]]:
    """RFC 4180 read, written here so the demo does not trust the producer's
    own reader: the round trip is only evidence if the two halves are
    independent."""
    rows: list[list[str]] = []
    row: list[str] = []
    cell = ""
    i = 0
    quoted = False
    started_quoted = False
    while i < len(text):
        c = text[i]
        if quoted:
            if c == '"':
                if i + 1 < len(text) and text[i + 1] == '"':
                    cell += '"'
                    i += 2
                    continue
                quoted = False
                i += 1
                continue
            cell += c
            i += 1
            continue
        if c == '"' and cell == "" and not started_quoted:
            quoted = True
            started_quoted = True
            i += 1
            continue
        if c == ",":
            row.append(cell)
            cell = ""
            started_quoted = False
            i += 1
            continue
        if c == "\r" and i + 1 < len(text) and text[i + 1] == "\n":
            row.append(cell)
            rows.append(row)
            row, cell, started_quoted = [], "", False
            i += 2
            continue
        cell += c
        i += 1
    if cell or row:
        row.append(cell)
        rows.append(row)
    return rows


def body() -> None:
    with RpcSubprocess(VIEWER, boot_grace=1.5) as d:
        # ── (A) the roster is readable before exporting ──────────────
        banner("A — a dialect says whether it can be faithful, before you pick it")
        roster: Any = d.query(f"{EXT}/export_dialects")
        ok("the roster is a list", isinstance(roster, list))
        names = [e["name"] for e in roster]
        assert_eq(names, ["comma", "tab", "clipboard"], "the three canonical dialects")
        by_name = {e["name"]: e for e in roster}
        ok("comma declares itself faithful", by_name["comma"]["faithful"] is True)
        ok("tab declares itself faithful", by_name["tab"]["faithful"] is True)
        ok(
            "clipboard declares itself LOSSY, up front",
            by_name["clipboard"]["faithful"] is False,
        )
        assert_eq(by_name["comma"]["terminator"], "crlf", "RFC 4180 line endings")
        assert_eq(by_name["tab"]["terminator"], "lf", "a tab block ends lines with LF")

        # ── (B) a faithful export of the whole capture loses nothing ─
        banner("B — the whole capture, exported faithfully, reads back cell for cell")
        every = export(d, "comma", "all")
        assert_eq(every["dialect"], "comma", "the answer names the dialect it used")
        assert_eq(every["scope"], "all", "and the scope")
        ok("a faithful dialect reports no loss", every["faithful"] is True)
        assert_eq(every["losses"], [], "and the loss list is empty, not absent")
        rows_all = every["rows"]
        ok("every captured row is in scope `all`", len(rows_all) == 16)

        block = read_back(every["text"])
        assert_eq(
            len(block),
            len(rows_all) + 1,
            "one header line plus one line per row",
        )
        widths = {len(line) for line in block}
        assert_eq(len(widths), 1, "every line has the same number of cells")

        # The cell that would have broken a naive export. Measured, not
        # assumed: the capture's list holds EXACTLY ONE comma-bearing cell (the
        # first count written here said seven, which was a count of comma-
        # bearing string literals in the whole fixture — most of them belong to
        # the decode tree, not to a row). One is enough, and an exact number is
        # a measurement where a floor is a guess.
        comma_cells = [c for line in block for c in line if "," in c]
        assert_eq(len(comma_cells), 1, "exactly one row cell holds a comma")
        ok(
            "a thousands separator did not split a column",
            "reassembled 3,144 B" in comma_cells[0],
        )
        ok(
            "and the writer actually quoted it rather than getting lucky",
            '"' in every["text"],
        )
        # ★★★★★ R2041 — DERIVED, where this said `block[7][5]`. That index was
        # where the reassembled row happened to sit, and R2041 moved it: the
        # capture's fragment run had been written upside down (`First` arriving
        # after `Last` in a newest-first table) and putting it right moved the
        # completing row two places up. A hand-written index encodes a position
        # and claims a relationship — so it fails when the data is corrected,
        # which is the direction that punishes a repair.
        #
        # The relationship is what this asks now: the note column of the row the
        # LIST shows that cell on is the cell the export wrote.
        # The relationship this asks now: the quoted cell is in the NOTE column
        # of exactly one line, and that line is a body line rather than the
        # header — so the quoting did not shift a cell into a neighbour's column
        # or fold two lines into one, which is what a naive writer does here.
        where = [
            (n, c)
            for n, line in enumerate(block)
            for c, cell in enumerate(line)
            if "," in cell
        ]
        assert_eq(len(where), 1, "exactly one cell in the block holds a comma")
        line_no, column = where[0]
        assert_eq(
            block[0][column],
            "name",
            "and it is in the column the row's own text is written in — the "
            "note rides inside that cell rather than in a column of its own",
        )
        ok(
            "the quoted cell is on a row line, not the header",
            0 < line_no <= len(rows_all),
        )

        # ── (C) the header line is the screen's own column titles ────
        banner("C — the header line names the columns this screen paints")
        head = block[0]
        assert_eq(head[0], "time", "the first column")
        assert_eq(head[3], "sn", "the sequence number column")
        assert_eq(head[6], "len", "the length column")
        ok("seven columns, matching the list", len(head) == 7)

        # ── (D) the verdict is about THIS content, not about the dialect ─
        banner("D — the report is content-specific, and this capture survives all three")
        # This is the honest outcome and the demo says so rather than
        # contriving a cell to break: a comma is not a tab block's delimiter, so
        # the very cell the comma dialect had to quote costs the tab dialects
        # nothing. A blanket "TSV may lose data" warning cannot tell those two
        # apart; a per-export verdict can, and that IS the deliverable.
        clip = export(d, "clipboard", "all")
        ok(
            "the LOSSY-CAPABLE dialect reports no loss on this capture",
            clip["faithful"] is True,
        )
        assert_eq(clip["losses"], [], "because nothing here holds a tab or a newline")
        ok(
            "even though the roster declared that dialect unable to be faithful",
            by_name["clipboard"]["faithful"] is False,
        )
        ok(
            "so `faithful` on the ANSWER and on the ROSTER are different claims",
            by_name["clipboard"]["faithful"] != clip["faithful"],
        )
        ok(
            "the tab dialect agrees, on the same content",
            export(d, "tab", "all")["faithful"] is True,
        )
        # And the shape survives whatever the verdict was.
        lines = clip["text"].split("\n")
        assert_eq(len(lines), len(rows_all) + 1, "the block keeps its line count")
        widths = {line.count("\t") for line in lines}
        assert_eq(len(widths), 1, "and its column count, on every line")
        ok(
            "the comma cell needed no quoting in a tab block",
            '"' not in clip["text"],
        )

        # ── (E) scope follows the filter ─────────────────────────────
        banner("E — `shown` is what the filter kept, and the answer says which rows")
        assert d.invoke(f"{EXT}/filter", "type = Data") is not None, "filter accepted"
        kept = d.query(f"{EXT}/kept_rows")
        assert_eq(len(kept), 9, "nine of the sixteen messages are of that kind")
        shown = export(d, "comma", "shown")
        assert_eq(shown["rows"], kept, "the export covers exactly the kept rows")
        shown_block = read_back(shown["text"])
        assert_eq(
            len(shown_block),
            len(kept) + 1,
            "header plus one line per kept row",
        )
        assert_eq(shown_block[0], block[0], "the same header line either way")
        ok(
            "`all` still covers the whole capture while a filter is on",
            len(export(d, "comma", "all")["rows"]) == 16,
        )
        # A second, numeric clause — the scope tracks the query rather than a
        # cached row set.
        assert d.invoke(f"{EXT}/filter", "len >= 1000") is not None, "second filter"
        assert_eq(len(export(d, "comma", "shown")["rows"]), 3, "three long messages")
        # The edge: a query that matches nothing is not a fault, and exporting
        # it yields the header line and no rows — never an empty file a reader
        # would take for a broken export.
        assert d.invoke(f"{EXT}/filter", "type = data") is not None, "case matters"
        assert_eq(d.query(f"{EXT}/query_fault"), None, "matching nothing is not a fault")
        empty = export(d, "comma", "shown")
        assert_eq(empty["rows"], [], "no rows kept")
        assert_eq(read_back(empty["text"]), [block[0]], "the header line, alone")
        ok("and an empty export is still faithful", empty["faithful"] is True)
        assert d.invoke(f"{EXT}/filter", "") is not None, "filter cleared"
        assert_eq(len(d.query(f"{EXT}/kept_rows")), 16, "every row is back")

        # ── (F) every refusal is a sentence ──────────────────────────
        banner("F — five ways to get the call wrong, five sentences")
        assert_action_refused(
            lambda: d.invoke(f"{EXT}/export", {"scope": "all"}),
            saying="no dialect named",
        )
        assert_action_refused(
            lambda: d.invoke(f"{EXT}/export", {"dialect": "semicolon", "scope": "all"}),
            saying="no dialect named",
        )
        assert_action_refused(
            lambda: d.invoke(f"{EXT}/export", {"dialect": "comma", "scope": "some"}),
            saying="no scope named",
        )
        assert_action_refused(
            lambda: d.invoke(f"{EXT}/export", {"dialect": "comma"}),
            saying="no scope named",
        )
        assert_action_refused(
            lambda: d.invoke(f"{EXT}/export", "comma"),
            saying="was given text",
        )
        ok("each refusal named which argument was wrong", True)

        # ── (G) the declaration is a precondition of dispatch ────────
        banner("G — the action publishes its grammar, and where each value comes from")
        fields = d.query(f"{EXT}/$schema")
        field = next(f for f in fields if f["path"] == "export")
        assert_eq(field["channel"], "invoke", "an invoke channel")
        assert_eq(field["arg_form"], {"kind": "object"}, "carried as an object")
        args = {a["name"]: a for a in field["args"]}
        assert_eq(sorted(args), ["dialect", "scope"], "two declared arguments")
        # The half a meta-object cannot express: not the argument's TYPE but
        # where its answerable values come from, so an agent enumerates a valid
        # call instead of guessing one.
        assert_eq(
            args["dialect"]["domain"],
            {"kind": "values_of", "values_path": "export_dialects"},
            "the dialect's values come from a path a client can read",
        )
        assert_eq(
            args["scope"]["domain"],
            {"kind": "one_of", "values": ["shown", "all"]},
            "the scope's values are a closed vocabulary, published",
        )
        ok(
            "and the path the domain points at is itself declared",
            any(f["path"] == "export_dialects" for f in fields),
        )
        ok(
            "the published scope words are the ones dispatch accepts",
            sorted(args["scope"]["domain"]["values"]) == sorted(["shown", "all"]),
        )

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1787 an export says what it could not carry", body)
