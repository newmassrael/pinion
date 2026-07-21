#!/usr/bin/env python3
"""R1402 §5.41 — the hex-dump's press-drag byte-range selection.

Generalises R1401's single-byte cursor into a drag SELECTION: a press latches an
anchor byte, each drag move extends the focus, and the range `[start, end)`
reverse-videos (R974) while the focus byte rings with a `GridCursor` (R975). A
plain click is the collapsed single-byte case. The `HexDumpOracle` owns the
selection; the router drives it via `pointer_move` + `invoke("send",
"PointerDown"/"PointerUp")`, so a real drag (`tf.drag`) exercises the whole
press → march → release arc — which also proves PointerDown reaches the oracle
(a second drag starts fresh instead of extending the first).

An AI client selects with no pixel via `invoke select_range "start,end"` and
reads it back at `selection_start/end/len/hex` — `selection_hex` is the bytes a
hex editor would copy. The proof is pure DATA over RPC: the selection's
reverse-video shows in `grid_rows` (`run.attrs.reverse`), cross-checked against
`selection_hex` (the painted bytes and the readout can never diverge).

Run from the workspace root:
    cargo build -p hello-hex-dump --release
    python3 tools/demos/r1402_hex_selection.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (660, 240)
GRID = "hex_dump"
ORACLE = "/external"
GRID_POS = (16, 40)
CELL_W, CELL_H = 8, 16


# --- helpers ---------------------------------------------------------------


def cursor(snap) -> dict:
    grid = find_by_tag(snap, GRID)
    assert grid is not None, "the hex-dump grid is in the paint scene"
    return grid["cursor"]


def run_at(row: dict, col: int) -> dict:
    for run in row["runs"]:
        if run["start"] <= col < run["start"] + run["len"]:
            return run
    raise AssertionError(f"column {col} is covered by some run")


def reverse_at(snap, col: int, row: int) -> bool:
    return run_at(find_by_tag(snap, GRID)["grid_rows"][row], col)["attrs"]["reverse"]


def hex_cell(tf, b: int) -> tuple[int, int]:
    col, row = tf.invoke(f"{ORACLE}/hex_cell", str(b)).split(",")
    return int(col), int(row)


def byte_xy(tf, b: int) -> tuple[int, int]:
    """The window pixel at byte `b`'s hex high-nibble cell centre."""
    col, row = hex_cell(tf, b)
    return (GRID_POS[0] + col * CELL_W + CELL_W // 2, GRID_POS[1] + row * CELL_H + CELL_H // 2)


def wait_focus(tf, b: int, desc: str):
    """Wait until the paint's cursor rings byte `b`'s hex cell."""
    col, row = hex_cell(tf, b)
    return wait_snap(
        tf,
        lambda s: cursor(s).get("visible") is True
        and cursor(s).get("col") == col
        and cursor(s).get("row") == row,
        source="paint",
        viewport=WIN,
        desc=desc,
    )


def assert_selection(tf, snap, start: int, end: int, focus: int) -> None:
    """The full selection witness: the queried range, the reverse-video on every
    selected byte's hex + ascii cells, the focus ring, and the hex readout
    cross-checked against the painted hex digits."""
    assert_eq(tf.query(f"{ORACLE}/selection_start"), start, "selection_start")
    assert_eq(tf.query(f"{ORACLE}/selection_end"), end, "selection_end")
    assert_eq(tf.query(f"{ORACLE}/selection_len"), end - start, "selection_len")
    assert_eq(tf.query(f"{ORACLE}/cursor_byte"), focus, "focus byte")
    # Every selected byte's hex pair + ascii cell reverse-video; the neighbours
    # just outside the range do not.
    painted = ""
    for b in range(start, end):
        hc, hr = hex_cell(tf, b)
        assert reverse_at(snap, hc, hr), f"byte {b} hex-hi reversed"
        assert reverse_at(snap, hc + 1, hr), f"byte {b} hex-lo reversed"
        ac, ar = (int(x) for x in tf.invoke(f"{ORACLE}/ascii_cell", str(b)).split(","))
        assert reverse_at(snap, ac, ar), f"byte {b} ascii reversed"
        painted += find_by_tag(snap, GRID)["grid_rows"][hr]["text"][hc : hc + 2]
    if start > 0:
        hc, hr = hex_cell(tf, start - 1)
        assert not reverse_at(snap, hc, hr), f"byte {start - 1} (before) not reversed"
    if end < 128:
        hc, hr = hex_cell(tf, end)
        assert not reverse_at(snap, hc, hr), f"byte {end} (past) not reversed"
    # The focus rings, and selection_hex equals the painted hex digits.
    assert_eq((cursor(snap)["col"], cursor(snap)["row"]), hex_cell(tf, focus), "focus ring")
    assert_eq(cursor(snap)["shape"], "block", "focus is a block cursor")
    assert_eq(tf.query(f"{ORACLE}/selection_hex"), painted, "selection_hex == painted")


def body() -> None:
    with RpcSubprocess("hello-hex-dump") as tf:
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == 78,
            source="paint",
            viewport=WIN,
            desc="hex-dump grid resolved",
        )

        # --- boot: no selection ---
        assert_eq(cursor(snap)["visible"], False, "boot: cursor hidden")
        assert_eq(tf.query(f"{ORACLE}/selection_start"), None, "boot: no selection")
        assert_eq(tf.query(f"{ORACLE}/selection_hex"), None, "boot: no hex")

        # --- AI-first no-pixel range: the header's length field [4, 8) ---
        assert_eq(
            tf.invoke(f"{ORACLE}/select_range", "4,8"), "4,8", "select_range returns the range"
        )
        witness = wait_focus(tf, 7, "select_range [4,8) rings focus 7")
        assert_selection(tf, witness, 4, 8, focus=7)
        assert_eq(tf.query(f"{ORACLE}/selection_hex"), "0000002c", "the length field bytes")
        # (Empty / out-of-range select_range rejects over RPC — the unit test
        # `select_range_invoke_and_selection_queries` covers InvokeError::Rejected.)

        # "none" clears it.
        tf.invoke(f"{ORACLE}/select_range", "none")
        gone = wait_snap(
            tf,
            lambda s: cursor(s).get("visible") is False,
            source="paint",
            viewport=WIN,
            desc="select_range none deselects",
        )
        assert_eq(cursor(gone)["visible"], False, "cleared")

        # --- a REAL press-drag: byte 4 -> byte 9 (same row) selects [4, 10) ---
        tf.drag(from_at=byte_xy(tf, 4), to_at=byte_xy(tf, 9))
        drag1 = wait_focus(tf, 9, "drag 4->9 rings focus 9")
        assert_selection(tf, drag1, 4, 10, focus=9)

        # --- a SECOND drag starts FRESH (proves PointerDown reaches the oracle:
        #     a missed press would extend the first selection instead) ---
        tf.drag(from_at=byte_xy(tf, 20), to_at=byte_xy(tf, 22))
        drag2 = wait_focus(tf, 22, "second drag 20->22 starts fresh")
        assert_selection(tf, drag2, 20, 23, focus=22)
        assert_eq(tf.query(f"{ORACLE}/selection_start"), 20, "not extending the first drag")

        # --- a plain click is a collapsed 1-byte selection (R1401 subsumed) ---
        tf.click(at=byte_xy(tf, 0))
        clicked = wait_focus(tf, 0, "click byte 0 = 1-byte selection")
        assert_selection(tf, clicked, 0, 1, focus=0)
        assert_eq(tf.query(f"{ORACLE}/selection_hex"), "50", "byte 0 = 0x50")

        # --- the no-pixel collapsed write still works (intervene cursor_byte) ---
        tf.intervene(f"{ORACLE}/cursor_byte", 15)
        iv = wait_focus(tf, 15, "intervene cursor_byte = 15")
        assert_selection(tf, iv, 15, 16, focus=15)


if __name__ == "__main__":
    sys.exit(run_demo("r1402_hex_selection", body))
