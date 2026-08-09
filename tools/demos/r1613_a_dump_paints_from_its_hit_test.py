#!/usr/bin/env python3
"""R1613 §5.16 §5.41 — a byte dump paints from the map that hit-tests it, and
a byte says WHICH named runs lit it.

The dump's geometry has been a crate type since R1606; its *paint* was still
55 lines inside `hello-hex-dump`, which made the two panes of an inspector
asymmetric — the detail tree came with a picture, the dump came with a map and
homework. R1613 moves the cell assembly into
`pinion_widget_paint::hex_dump::view_hex_dump`, and the move is not a
relocation: the painter now asks `HexLayout::region_at` what each cell is and
`HexLayout::glyph_at` what it shows, which are the same two questions a
pointer asks. What is drawn and what responds are one fact.

Two things become checkable over the wire because of that, and this demo
checks both without a pixel:

  * **The glyphs are derived.** Every row's offset field is that row's own
    start offset, every bar is a `|`, and every hex pair is the byte the
    hit-test says lives in that cell. The example used to format the offset to
    a fixed eight digits and truncate, so a narrower offset column printed
    `0000` on every row; the derivation has no such seam.
  * **A byte says why it is lit.** The two gestures — the overview brush's
    field and the drag selection — are now *named* runs
    (`MarkSet`), not colours, so `/external/marks_at` answers with the stack
    covering a byte in the order that decided the paint. A list of coloured
    ranges cannot answer that: by the time anyone asks, only the colour is
    left, and two runs that resolve to the same ink are indistinguishable.

The overlap rule is one direction — later-declared wins, for every visual
channel alike — and the paint is asserted against it: the field fills, the
selection reverses, and a byte in both carries the fill AND the reverse
because the selection says nothing about colour.

Run from the workspace root:
    cargo build -p hello-hex-dump --release
    python3 tools/demos/r1613_a_dump_paints_from_its_hit_test.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (660, 240)
GRID = "hex_dump"
BRUSH = "/hex_brush/external"

BYTE_COUNT = 128
BYTES_PER_ROW = 16
TOTAL_COLS = 78
ROWS = 8
OFFSET_DIGITS = 8


# --- snapshot helpers ------------------------------------------------------


def grid(snap) -> dict:
    node = find_by_tag(snap, GRID)
    assert node is not None, "the hex-dump grid is in the paint scene"
    return node


def rows_of(snap) -> list:
    return grid(snap)["grid_rows"]


def run_at(row: dict, col: int) -> dict:
    """The style run covering `col` (`grid_rows` is RLE by style)."""
    for run in row["runs"]:
        if run["start"] <= col < run["start"] + run["len"]:
            return run
    raise AssertionError(f"column {col} is covered by some run")


def bg_kind(row: dict, col: int) -> str:
    return run_at(row, col)["bg"]["kind"]


def reversed_at(row: dict, col: int) -> bool:
    return run_at(row, col)["attrs"]["reverse"]


# --- oracle helpers --------------------------------------------------------


def cell_of(tf, which: str, b: int) -> tuple[int, int]:
    col, row = tf.invoke(f"/external/{which}", str(b)).split(",")
    return int(col), int(row)


def byte_at(tf, col: int, row: int) -> str:
    return tf.invoke("/external/byte_at_cell", f"{col},{row}")


def byte_window(tf, low: float, high: float) -> tuple[int, int]:
    lo, hi = tf.invoke("/external/byte_window", f"{low},{high}").split(",")
    return int(lo), int(hi)


def marks_at(tf, b: int, lo: int, hi: int) -> str:
    """The named runs covering byte `b` when the field is `[lo, hi)`.

    The field belongs to the sibling brush external rather than to the dump's
    own oracle, so the caller names it — which is exactly the seam, stated.
    """
    return tf.invoke("/external/marks_at", f"{b},{lo},{hi}")


def set_brush(tf, low: float, high: float) -> tuple[int, int, object]:
    """Drive the brush and wait until the paint settles to the byte window the
    oracle reports for it. No fixed sleep."""
    tf.intervene(f"{BRUSH}/high", high)
    tf.intervene(f"{BRUSH}/low", low)
    box: dict = {}

    def settled(s) -> bool:
        cur_low = tf.query(f"{BRUSH}/low")
        cur_high = tf.query(f"{BRUSH}/high")
        if abs(cur_low - low) > 0.02 or abs(cur_high - high) > 0.02:
            return False
        lo, hi = byte_window(tf, cur_low, cur_high)
        box["lo"], box["hi"] = lo, hi
        node = find_by_tag(s, GRID)
        if node is None:
            return False
        tinted = sum(
            run["len"]
            for row in node["grid_rows"]
            for run in row["runs"]
            if run["bg"]["kind"] == "rgb"
        )
        return tinted == 3 * (hi - lo)

    snap = wait_snap(
        tf,
        settled,
        source="paint",
        viewport=WIN,
        desc=f"brush {low:.3f}..{high:.3f} settles",
    )
    return box["lo"], box["hi"], snap


def select(tf, start: int, end: int):
    """Drive the drag selection and wait for the reverse-video to appear."""
    tf.invoke("/external/select_range", f"{start},{end}")

    def settled(s) -> bool:
        node = find_by_tag(s, GRID)
        if node is None:
            return False
        rev = sum(
            run["len"]
            for row in node["grid_rows"]
            for run in row["runs"]
            if run["attrs"]["reverse"]
        )
        return rev == 3 * (end - start)

    return wait_snap(
        tf,
        settled,
        source="paint",
        viewport=WIN,
        desc=f"selection {start}..{end} reverses",
    )


def body() -> None:
    with RpcSubprocess("hello-hex-dump") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, GRID) is not None
            and find_by_tag(s, GRID)["cols"] == TOTAL_COLS
            and len(find_by_tag(s, GRID)["grid_rows"]) == ROWS,
            source="paint",
            viewport=WIN,
            desc="the dump's grid resolves to its layout",
        )
        rows = rows_of(snap)
        assert_eq(grid(snap)["cols"], TOTAL_COLS, "the grid is the layout's width")
        assert_eq(len(rows), ROWS, "the grid is the layout's height")
        assert_eq(tf.query("/external/total_cols"), TOTAL_COLS, "oracle agrees on width")
        assert_eq(tf.query("/external/row_count"), ROWS, "oracle agrees on height")

        # --- the glyphs are DERIVED, not assembled beside the map -----------
        #
        # Every row's offset field is that row's own start offset, at the
        # layout's declared width. The paint that this replaced formatted to a
        # fixed eight digits and then truncated, which is invisible at eight
        # and wrong at any other width.
        for r in range(ROWS):
            text = rows[r]["text"]
            assert_eq(
                text[:OFFSET_DIGITS],
                f"{r * BYTES_PER_ROW:08x}",
                f"row {r} offset field is its own start offset",
            )

        # Both gutter bars are `|`, on every row, and neither is a byte.
        left, right = 60, 77
        for r in range(ROWS):
            assert_eq(rows[r]["text"][left], "|", f"row {r} left bar")
            assert_eq(rows[r]["text"][right], "|", f"row {r} right bar")
        assert_eq(byte_at(tf, left, 0), "none", "a bar is not a byte")
        assert_eq(byte_at(tf, 0, 0), "none", "an offset digit is not a byte")
        assert_eq(byte_at(tf, 9, 0), "none", "the gap before the hex column is not a byte")

        # A hex cell draws the byte the hit-test says lives there. The byte's
        # value is read from the oracle (`select_range` + `selection_hex`), so
        # nothing here is guessed from the picture.
        for b in (0, 3, 8, 15, 16, 63, 127):
            hcol, hrow = cell_of(tf, "hex_cell", b)
            acol, arow = cell_of(tf, "ascii_cell", b)
            assert_eq(byte_at(tf, hcol, hrow), str(b), f"byte {b} hex cell hit-tests to it")
            assert_eq(byte_at(tf, hcol + 1, hrow), str(b), f"byte {b} low nibble too")
            assert_eq(byte_at(tf, acol, arow), str(b), f"byte {b} ascii cell hit-tests to it")
            select(tf, b, b + 1)
            value = tf.query("/external/selection_hex")
            assert_eq(
                rows_of(tf.snapshot(source="paint", viewport=WIN))[hrow]["text"][hcol : hcol + 2],
                value,
                f"byte {b} draws its own two hex digits",
            )

        # The ascii gutter draws the byte or a `.`, and never anything else.
        snap = tf.snapshot(source="paint", viewport=WIN)
        rows = rows_of(snap)
        header = rows[0]["text"]
        acol0, _ = cell_of(tf, "ascii_cell", 0)
        assert_eq(header[acol0 : acol0 + 3], "PIN", "the magic reads in the gutter")
        acol3, _ = cell_of(tf, "ascii_cell", 3)
        assert_eq(header[acol3], ".", "a non-printable byte is a dot")

        # --- a byte says WHY it is lit --------------------------------------
        lo, hi, _ = set_brush(tf, 8 / BYTE_COUNT, 16 / BYTE_COUNT)
        assert_eq((lo, hi), (8, 16), "the field run is bytes 8..16")
        snap = select(tf, 12, 20)
        rows = rows_of(snap)

        assert_eq(marks_at(tf, 9, lo, hi), "field", "9 is in the brush field alone")
        assert_eq(marks_at(tf, 13, lo, hi), "field,selection", "13 is in both")
        assert_eq(marks_at(tf, 18, lo, hi), "selection", "18 is in the drag alone")
        assert_eq(marks_at(tf, 40, lo, hi), "none", "40 is in neither")
        assert_eq(marks_at(tf, 8, lo, hi), "field", "the field's first byte")
        assert_eq(marks_at(tf, 7, lo, hi), "none", "the byte before it is out")
        assert_eq(marks_at(tf, 19, lo, hi), "selection", "the drag's last byte")
        assert_eq(marks_at(tf, 20, lo, hi), "none", "one past the drag is out")

        # The names come back innermost-LAST, which is the order the paint
        # resolves in: one direction, every channel alike.
        assert_eq(
            marks_at(tf, 13, lo, hi).split(",")[-1],
            "selection",
            "the last name is the one the painter obeyed",
        )

        # And the paint agrees with the answer, cell by cell. The field fills
        # (an `rgb` background); the selection reverses; a byte in both carries
        # BOTH, because the selection says nothing about colour.
        def channels(b: int) -> tuple[bool, bool]:
            hcol, hrow = cell_of(tf, "hex_cell", b)
            acol, arow = cell_of(tf, "ascii_cell", b)
            filled = bg_kind(rows[hrow], hcol) == "rgb"
            rev = reversed_at(rows[hrow], hcol)
            # The two disjoint regions of one byte stay in lockstep.
            assert_eq(bg_kind(rows[arow], acol) == "rgb", filled, f"byte {b} ascii fill")
            assert_eq(reversed_at(rows[arow], acol), rev, f"byte {b} ascii reverse")
            assert_eq(bg_kind(rows[hrow], hcol + 1) == "rgb", filled, f"byte {b} low fill")
            return filled, rev

        assert_eq(channels(9), (True, False), "field only: filled, not reversed")
        assert_eq(channels(13), (True, True), "both: filled AND reversed")
        assert_eq(channels(18), (False, True), "drag only: reversed, not filled")
        assert_eq(channels(40), (False, False), "neither: plain")

        # The channels the query names are the channels the paint has -- for
        # every byte of the buffer, not just the sampled ones.
        for b in range(BYTE_COUNT):
            names = marks_at(tf, b, lo, hi)
            filled, rev = channels(b)
            assert_eq(filled, "field" in names, f"byte {b} fill follows its marks")
            assert_eq(rev, "selection" in names, f"byte {b} reverse follows its marks")

        # --- the runs move, and the answer moves with them ------------------
        lo, hi, _ = set_brush(tf, 64 / BYTE_COUNT, 80 / BYTE_COUNT)
        assert_eq((lo, hi), (64, 80), "the field moved to the fourth row")
        snap = select(tf, 70, 74)
        rows = rows_of(snap)
        assert_eq(marks_at(tf, 13, lo, hi), "none", "the old field is gone")
        assert_eq(marks_at(tf, 65, lo, hi), "field", "the new field is here")
        assert_eq(marks_at(tf, 71, lo, hi), "field,selection", "and the drag inside it")
        assert_eq(channels(13), (False, False), "the old bytes are plain again")
        assert_eq(channels(71), (True, True), "the new overlap carries both")

        # Clearing the drag leaves the field alone -- two independent runs.
        tf.invoke("/external/select_range", "none")
        snap = wait_snap(
            tf,
            lambda s: not any(
                run["attrs"]["reverse"]
                for row in find_by_tag(s, GRID)["grid_rows"]
                for run in row["runs"]
            ),
            source="paint",
            viewport=WIN,
            desc="the drag clears",
        )
        rows = rows_of(snap)
        assert_eq(marks_at(tf, 71, lo, hi), "field", "only the field is left")
        assert_eq(channels(71), (True, False), "filled, no longer reversed")
        assert_eq(
            tf.query("/external/selection_start"),
            None,
            "the oracle reports no selection",
        )

        # --- what the query rejects -----------------------------------------
        assert_action_refused(
            lambda: tf.invoke("/external/marks_at", "999,0,8"),
            saying="no byte 999 in this buffer",
        )
        assert_rpc_error(
            lambda: tf.invoke("/external/marks_at", "0,8"),
            data="InvokeTypeMismatch",
        )
        assert_rpc_error(
            lambda: tf.invoke("/external/marks_at", 7),
            data="InvokeTypeMismatch",
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1613_a_dump_paints_from_its_hit_test", body))
