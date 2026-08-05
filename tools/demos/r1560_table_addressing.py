#!/usr/bin/env python3
"""R1560 §5.36 §5.12 §5.40 — a cell is addressed by its place in the flow.

Everything else a table has can be written by hand: a border is a stroke, a
padding is an inset, a column width is a length. A cell's ADDRESS cannot,
because it is not a property of the cell — it is where the cell lands once
every earlier cell's spans have taken their slots. Insert one cell and every
cell after it re-addresses. That is what R1560 derives, and what this demo
asserts against a real application, over the wire.

  * `scene/text_tables` publishes the structure: one row per table with its
    shape, its column tracks, the slots nobody filled, and every cell's address
    and PAINTED box. Qt has no peer for any of it — a `QTextTable` exists only
    inside a `QTextDocument`, `QTextTableCell::row()` is an in-process C++
    call, there is no accessor that enumerates a document's tables, and a
    cell's rect has to be reconstructed from cursor positions;
  * the Toggle INSERTS a cell into the middle of the table, and every cell
    after it moves a whole row down while the cells before it do not. Nothing
    in the binding states an address, so this is the derivation and not an
    author;
  * that inserted cell asks for NINE columns in a three-column table. Qt's
    `mergeCells` returns `void` and silently does nothing when a merge does not
    fit, so the ask and the result are afterwards indistinguishable; here the
    span is clamped to the free run and both numbers are published, with
    `clamped` naming the difference;
  * a cell spans two rows, so the next row's cells step around it — the case a
    nest of flex rows cannot express at all;
  * Thursday asks for exactly the table's WIDTH and gets one column, because
    the second room booking reaches into its row. That is the clamp against the
    free run rather than against the row's remainder — two different numbers,
    and the only pair that tells the two rules apart;
  * the last row's topic slot has no cell. `QTextTable` cannot be in that
    state: `insertRows` fills its grid;
  * header-ness is DERIVED from the address. One `header_rows` / `header_columns`
    declaration, and the corner, the column labels and the row labels are all
    headers because of where they landed — including after the insert;
  * `scene/access` carries WAI-ARIA `table` / `row` / `cell` with
    `aria-rowindex` / `aria-colindex` / `aria-rowspan` and the
    `columnheader` / `rowheader` bands. A `QTextTable` reaches no accessibility
    interface at all;
  * `scene/snapshot` carries the same derivation on each paragraph, so the two
    introspection channels check each other rather than restating one;
  * a paragraph outside every table is not a cell — the negative control that
    separates "the feature published a table" from "the method lists text";
  * `rpc/schema` describes the four new types, so the R1539 census covers this
    round's wire the day it lands.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-richtext-cells --release
    python3 tools/demos/r1560_table_addressing.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-richtext-cells"
VIEWPORT = (620, 520)

DOC_TAG = "week"
TOGGLE_TAG = "main_toggle"

# What the binding declares. The demo asserts the wire reports THESE rather
# than whatever the layout happened to produce.
COLUMNS = 3
DAY_COL_PX = 90
CELL_PADDING_PX = 6
NOTE_DECLARED_SPAN = 9
THU_DECLARED_SPAN = 3

# The paragraphs, by their text — addressed by CONTENT rather than by index,
# because inserting the note shifts every later index and that shift is the
# behaviour under test.
H1_TEXT = "This week"
INTRO_TEXT = "Rooms are held for the whole booking."
HEAD_DAY = "Day"
HEAD_ROOM = "Room"
HEAD_TOPIC = "Topic"
MON = "Mon"
ROOM_A = "A1"
TOPIC_KICKOFF = "Kickoff"
KICKOFF_NOTE = "bring the printed agenda"
TUE = "Tue"
TOPIC_DESIGN = "Design review"
NOTE_TEXT = "Thursday is provisional."
WED = "Wed"
ROOM_B = "B2"
TOPIC_WRAP = "Wrap-up"
THU = "Thu"

# The exact published shape of one table row and one cell. A response an agent
# is told to rely on should not be able to gain or lose a key unnoticed (the
# R1538 lesson, which R1539 turned into a gate).
TABLE_KEYS = {
    "tag",
    "rows",
    "columns",
    "cell_count",
    "header_rows",
    "header_columns",
    "column_widths",
    "cell_padding_px",
    "cell_spacing_px",
    "border_px",
    "x",
    "y",
    "width",
    "height",
    "slack",
    "cells",
}
CELL_KEYS = {
    "tag",
    "row_tag",
    "row",
    "column",
    "row_span",
    "column_span",
    "declared_row_span",
    "declared_column_span",
    "clamped",
    "header",
    "index",
    "blocks",
    "x",
    "y",
    "width",
    "height",
}
PLACEMENT_KEYS = {
    "table_tag",
    "cell_tag",
    "row_tag",
    "row",
    "column",
    "row_span",
    "column_span",
    "declared_column_span",
    "declared_row_span",
    "row_count",
    "column_count",
    "header",
    "index",
    "opens_cell",
    "format",
}


def tables(tf: RpcSubprocess) -> list[dict[str, Any]]:
    return call(tf, "scene/text_tables")["tables"]


def text_of_tag(snap: Any, tag: str) -> str:
    node = find_by_tag(snap, tag)
    assert node is not None, f"no painted node tagged {tag}"
    return node.get("content", "")


def addresses(tf: RpcSubprocess) -> dict[str, tuple[int, int]]:
    """Every cell's first paragraph text mapped to its derived address.

    Joined through the paragraph tag, which is the key `scene/snapshot`,
    `scene/text_tables` and `scene/access` all address the same object by.
    """
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    out: dict[str, tuple[int, int]] = {}
    for table in tables(tf):
        for cell in table["cells"]:
            out[text_of_tag(snap, cell["blocks"][0])] = (cell["row"], cell["column"])
    return out


def cell_named(table: dict[str, Any], tf: RpcSubprocess, text: str) -> dict[str, Any]:
    """The published cell whose first paragraph reads `text`."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    for cell in table["cells"]:
        if text_of_tag(snap, cell["blocks"][0]) == text:
            return cell
    raise AssertionError(f"no cell begins with {text!r}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # A paint scene is what every surface below reads, so ask for one first.
        wait_until(
            lambda: find_by_tag(
                tf.snapshot(source="paint", viewport=VIEWPORT), f"{DOC_TAG}_tbl0"
            )
            is not None,
            timeout=5.0,
            interval=0.03,
            desc="the document is painted",
        )

        # ── (A) the STRUCTURE: one table, the shape the allocation gave it ──
        rows = tables(tf)
        assert_eq(len(rows), 1, f"A: one table on screen: {[r['tag'] for r in rows]}")
        table = rows[0]
        assert_eq(set(table), TABLE_KEYS, "A: a table row's exact key set")
        assert_eq(set(table["cells"][0]), CELL_KEYS, "A: a cell's exact key set")
        assert_eq(table["tag"], f"{DOC_TAG}_tbl0", "A: tables are addressable")
        assert_eq(table["columns"], COLUMNS, "A: the width it declared")
        assert_eq(
            table["rows"],
            5,
            "A: and the height nobody declared — the allocation needed five "
            "rows for twelve cells one of which spans two",
        )
        assert_eq(table["cell_count"], 12, "A: twelve cells")
        assert_eq(table["cell_padding_px"], CELL_PADDING_PX, "A: its metrics")
        assert_eq(table["border_px"], 1)
        assert table["height"] is not None and table["height"] > 0, (
            "A: and the box the layout gave it"
        )

        # ── (B) the ADDRESSES, none of which the binding wrote ─────────────
        before = addresses(tf)
        assert_eq(before[HEAD_DAY], (0, 0), "B: the flow starts at the origin")
        assert_eq(before[HEAD_ROOM], (0, 1))
        assert_eq(before[HEAD_TOPIC], (0, 2))
        assert_eq(before[MON], (1, 0), "B: and wraps at the declared width")
        assert_eq(before[WED], (3, 0))
        assert_eq(before[THU], (4, 0))

        # ── (C) a row span reaches into a row nobody has written yet ───────
        booking = cell_named(table, tf, ROOM_A)
        assert_eq(booking["row_span"], 2, "C: the room is booked for two days")
        assert_eq((booking["row"], booking["column"]), (1, 1))
        assert_eq(
            before[TUE],
            (2, 0),
            "C: Tuesday's label takes the first free slot of the next row",
        )
        assert_eq(
            before[TOPIC_DESIGN],
            (2, 2),
            "C: and its topic steps around the booking, which holds column 1 — "
            "the case a nest of flex rows cannot express",
        )

        # ── (D) header-ness is DERIVED from the address ────────────────────
        assert_eq(table["header_rows"], 1, "D: one declaration for the row band")
        assert_eq(table["header_columns"], 1, "D: one for the column band")
        assert_eq(
            cell_named(table, tf, HEAD_DAY)["header"],
            "corner",
            "D: the corner cell is in both bands",
        )
        assert_eq(cell_named(table, tf, HEAD_ROOM)["header"], "column")
        assert_eq(cell_named(table, tf, MON)["header"], "row")
        assert_eq(
            cell_named(table, tf, TOPIC_KICKOFF)["header"],
            "none",
            "D: and a data cell is neither — nothing declared any of these",
        )

        # ── (E) a multi-block cell is one cell ─────────────────────────────
        kickoff = cell_named(table, tf, TOPIC_KICKOFF)
        assert_eq(len(kickoff["blocks"]), 2, "E: two paragraphs in one cell")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(
            text_of_tag(snap, kickoff["blocks"][1]),
            KICKOFF_NOTE,
            "E: the continuation joined the box its opener made",
        )

        # ── (F) the tracks, in CSS's spelling, and the columns they made ───
        assert_eq(
            table["column_widths"],
            [f"{DAY_COL_PX}px", "1fr", "2fr"],
            "F: the declared tracks, resolved to one per column",
        )
        day = cell_named(table, tf, HEAD_DAY)
        room = cell_named(table, tf, HEAD_ROOM)
        topic = cell_named(table, tf, HEAD_TOPIC)
        assert_eq(
            room["x"] - day["x"],
            DAY_COL_PX,
            "F: the fixed column is the width it was declared",
        )
        assert (topic["width"] or 0) > (room["width"] or 0), (
            f"F: the 2fr column is wider than the 1fr one "
            f"({topic['width']} vs {room['width']})"
        )
        assert_eq(
            cell_named(table, tf, ROOM_B)["x"],
            room["x"],
            "F: and every cell in a column starts at that column's edge, which "
            "is what a grid does and a column of flex rows cannot",
        )

        # ── (G) the ragged tail: slots nobody filled ───────────────────────
        assert_eq(
            [(s["row"], s["column"]) for s in table["slack"]],
            [(4, 2)],
            "G: the week has no topic for Thursday — a state QTextTable "
            "cannot be in, because insertRows fills its grid",
        )

        # ── (G2) the OTHER clamp, and the one that tells the rule apart ─────
        thu = cell_named(table, tf, THU)
        assert_eq(
            thu["declared_column_span"],
            THU_DECLARED_SPAN,
            "G2: Thursday asks for exactly the table's width, so a clamp "
            "against the row's REMAINDER would grant it in full",
        )
        assert_eq(
            thu["column_span"],
            1,
            "G2: and it gets one column, because the second room is booked "
            "into its row — the span is clamped to the FREE RUN, which is the "
            "only rule under which no two cells can share a slot",
        )
        assert_eq(thu["clamped"], True)
        booking_b = cell_named(table, tf, ROOM_B)
        assert_eq(
            (booking_b["row"], booking_b["column"], booking_b["row_span"]),
            (3, 1, 2),
            "G2: which is the booking that holds the slot beside Thursday",
        )

        # ── (H) snapshot carries the same derivation ───────────────────────
        mon_tag = cell_named(table, tf, MON)["blocks"][0]
        mon_node = find_by_tag(snap, mon_tag)
        assert mon_node is not None, "H: the paragraph is painted"
        placement = mon_node["cell"]
        assert_eq(set(placement), PLACEMENT_KEYS, "H: the placement's key set")
        assert_eq(
            (placement["row"], placement["column"]),
            before[MON],
            "H: and it is the SAME address the census published — two "
            "channels reading one derivation, not two derivations",
        )
        assert_eq(placement["opens_cell"], True)
        label_node = find_by_tag(snap, f"{DOC_TAG}_blk1")
        assert_eq(
            label_node["cell"],
            None,
            "H: an ordinary paragraph is null, so a reader tells a cell from "
            "any other text without guessing",
        )

        # ── (I) assistive technology hears the whole structure ─────────────
        access = call(tf, "scene/access")["nodes"]
        by_tag = {n["tag"]: n for n in access}
        assert f"{DOC_TAG}_tbl0" in by_tag, (
            "I: the table reaches the announced tree AT ALL — the pass that "
            "derives it has to be wired into the assembler, and a unit test "
            "that calls the pass itself cannot see that nobody calls it "
            "(R1559's lesson)"
        )
        table_node = by_tag[f"{DOC_TAG}_tbl0"]
        assert_eq(table_node["role"], "table", "I: a document table, not a grid")
        assert_eq(table_node["row_count"], 5, "I: aria-rowcount")
        assert_eq(table_node["column_count"], COLUMNS, "I: aria-colcount")
        booking_node = by_tag[booking["tag"]]
        assert_eq(booking_node["role"], "cell")
        assert_eq(booking_node["row_index"], 2, "I: aria-rowindex is 1-based")
        assert_eq(booking_node["column_index"], 2, "I: aria-colindex too")
        assert_eq(
            booking_node["row_span"],
            2,
            "I: and the merged extent, without which every cell after it is "
            "heard in the wrong place",
        )
        assert_eq(
            booking_node["name"],
            ROOM_A,
            "I: named from the text that was PAINTED in it",
        )
        assert_eq(
            by_tag[cell_named(table, tf, MON)["tag"]]["role"],
            "rowheader",
            "I: the day column is a header band",
        )
        assert_eq(by_tag[cell_named(table, tf, HEAD_ROOM)["tag"]]["role"], "columnheader")
        assert_eq(
            by_tag[booking["row_tag"]]["role"],
            "row",
            "I: and the rows are real announced objects",
        )

        # ── (J) THE WHOLE POINT: one insertion re-addresses what follows ───
        tf.click(path=TOGGLE_TAG)
        wait_until(
            lambda: tables(tf)[0]["rows"] == 6,
            timeout=4.0,
            interval=0.03,
            desc="the note enters the table",
        )
        after = addresses(tf)
        assert_eq(after[HEAD_DAY], (0, 0), "J: the header did not move")
        assert_eq(after[TOPIC_DESIGN], (2, 2), "J: nor did anything before the note")
        assert_eq(after[NOTE_TEXT], (3, 0), "J: the note took the next free slot")
        assert_eq(
            after[WED],
            (4, 0),
            "J: and every cell after it moved a whole row down — nothing the "
            "binding wrote states an address, so this is the derivation",
        )
        assert_eq(after[TOPIC_WRAP], (4, 2))
        assert_eq(after[THU], (5, 0))
        assert_eq(before[WED], (3, 0), "J: which is one row up from where it was")

        # ── (K) the impossible span: clamped, and the ask still readable ───
        noted = tables(tf)[0]
        note = cell_named(noted, tf, NOTE_TEXT)
        assert_eq(
            note["declared_column_span"],
            NOTE_DECLARED_SPAN,
            "K: the binding asked for nine columns",
        )
        assert_eq(note["column_span"], COLUMNS, "K: and got the three that exist")
        assert_eq(
            note["clamped"],
            True,
            "K: with the difference NAMED — Qt's mergeCells returns void, so a "
            "refused merge leaves no trace at all",
        )
        assert_eq(
            cell_named(noted, tf, WED)["clamped"],
            False,
            "K: and an ordinary cell is not reported as clamped",
        )
        assert_eq(
            [(s["row"], s["column"]) for s in noted["slack"]],
            [(5, 2)],
            "K: the ragged tail moved down with everything else",
        )
        assert_eq(
            cell_named(noted, tf, HEAD_DAY)["header"],
            "corner",
            "K: and the header bands still fall where the declaration says, "
            "because they are a function of the address",
        )

        # ── (L) the R1539 census covers this round's wire ──────────────────
        schema = call(tf, "rpc/schema")
        by_name = {t["name"]: t for t in schema["types"]}
        for name in (
            "TextTableWire",
            "TextCellWire",
            "GridSlotWire",
            "TextTablesOutcome",
        ):
            assert name in by_name, f"L: {name} is described by the published census"
        assert_eq(
            {f["name"] for f in by_name["TextTableWire"]["shape"]["fields"]},
            TABLE_KEYS,
            "L: and the census's key set is the one the live wire answered with",
        )
        assert_eq(
            {f["name"] for f in by_name["TextCellWire"]["shape"]["fields"]},
            CELL_KEYS,
            "L: for a cell too",
        )
        height_field = next(
            f for f in by_name["TextCellWire"]["shape"]["fields"] if f["name"] == "height"
        )
        assert_eq(
            height_field["nullable"],
            True,
            "L: the census states that a box may be null, which is the "
            "not-yet-laid-out case an agent must handle",
        )

        # ── (M) the method is discoverable and does not mutate ─────────────
        methods = call(tf, "rpc/methods")["methods"]
        entry = next(m for m in methods if m["name"] == "scene/text_tables")
        assert_eq(entry["occ"], "read", "M: reading a table mutates nothing")
        assert_eq(
            addresses(tf),
            after,
            "M: and the document is where (J) left it",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1560 §5.36 a cell is addressed by its place", body))
