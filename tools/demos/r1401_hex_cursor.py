#!/usr/bin/env python3
"""R1401 §5.41 — the hex-dump's click-a-byte cursor.

Extends `hello-hex-dump` (R1400's field brush) with a single-byte inspect: a
click on a byte's hex or ascii cell resolves the byte under it (via
`byte_at_cell`, the inverse of the R1400 hex_cell / ascii_cell mapping, reusing
the R1008 `CellMetric::px_to_cell` pointer->cell substrate as its second
consumer), and the view rings that byte with a `GridCursor` (R975). The
`HexDumpOracle` owns the cursor: a real click drives its `pointer_move`, and an
AI client drives it with no pixel via `scene/intervene /external/cursor_byte`
(Int = select, null = deselect), reading it back at `cursor_byte` /
`cursor_value` / `cursor_cell`.

The proof is pure DATA over RPC (no OCR): the grid's `cursor` object
(`{col, row, shape, visible}`) reports where the ring lands, and the oracle's
`cursor_value` is cross-checked against the hex digits the grid actually paints
at that cell — so the inspected value and the painted value can never diverge.

Run from the workspace root:
    cargo build -p hello-hex-dump --release
    python3 tools/demos/r1401_hex_cursor.py
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
GRID_POS = (16, 40)  # the grid's absolute window origin
CELL_W, CELL_H = 8, 16  # CellMetric::DEFAULT


# --- helpers ---------------------------------------------------------------


def cursor(snap) -> dict:
    grid = find_by_tag(snap, GRID)
    assert grid is not None, "the hex-dump grid is in the paint scene"
    return grid["cursor"]


def hex_cell(tf, b: int) -> tuple[int, int]:
    """The oracle's hex cell for byte `b` — `(col, row)`."""
    col, row = tf.invoke(f"{ORACLE}/hex_cell", str(b)).split(",")
    return int(col), int(row)


def byte_click_xy(tf, b: int) -> tuple[int, int]:
    """The window pixel at the centre of byte `b`'s hex high-nibble cell."""
    col, row = hex_cell(tf, b)
    return (GRID_POS[0] + col * CELL_W + CELL_W // 2, GRID_POS[1] + row * CELL_H + CELL_H // 2)


def painted_hex(snap, b: int, col: int, row: int) -> str:
    """The two hex digits the grid actually paints for byte `b` at `(col,row)`."""
    return find_by_tag(snap, GRID)["grid_rows"][row]["text"][col : col + 2]


def click_byte(tf, b: int) -> tuple[dict, int, int]:
    """Click byte `b`'s hex cell and wait until the ring lands there."""
    col, row = hex_cell(tf, b)
    tf.click(at=byte_click_xy(tf, b))
    snap = wait_snap(
        tf,
        lambda s: cursor(s).get("visible") is True
        and cursor(s).get("col") == col
        and cursor(s).get("row") == row,
        source="paint",
        viewport=WIN,
        desc=f"cursor rings byte {b} at ({col},{row})",
    )
    return snap, col, row


def assert_ringed(tf, snap, b: int, col: int, row: int) -> None:
    """The full cursor witness: the ring, the queried byte, and the inspected
    value cross-checked against the painted hex."""
    cur = cursor(snap)
    assert cur["visible"], f"byte {b} cursor visible"
    assert_eq(cur["shape"], "block", f"byte {b} cursor is a block")
    assert_eq((cur["col"], cur["row"]), (col, row), f"byte {b} cursor cell")
    assert_eq(tf.query(f"{ORACLE}/cursor_byte"), b, f"byte {b} queried")
    assert_eq(tf.query(f"{ORACLE}/cursor_cell"), f"{col},{row}", f"byte {b} cursor_cell")
    # The inspected value equals the hex the grid paints at that cell.
    value = tf.query(f"{ORACLE}/cursor_value")
    assert_eq(value, painted_hex(snap, b, col, row), f"byte {b} value == painted hex")


def body() -> None:
    with RpcSubprocess("hello-hex-dump") as tf:
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == 78,
            source="paint",
            viewport=WIN,
            desc="hex-dump grid resolved",
        )

        # --- boot: no cursor ---
        assert_eq(cursor(snap)["visible"], False, "boot: cursor hidden")
        assert_eq(tf.query(f"{ORACLE}/cursor_byte"), None, "boot: cursor_byte null")
        assert_eq(tf.query(f"{ORACLE}/cursor_value"), None, "boot: cursor_value null")
        assert_eq(tf.query(f"{ORACLE}/cursor_cell"), None, "boot: cursor_cell null")

        # --- byte_at_cell: the inverse of hex_cell / ascii_cell ---
        hcol, hrow = hex_cell(tf, 5)
        assert_eq(tf.invoke(f"{ORACLE}/byte_at_cell", f"{hcol},{hrow}"), "5", "hex cell -> byte 5")
        assert_eq(
            tf.invoke(f"{ORACLE}/byte_at_cell", f"{hcol + 1},{hrow}"), "5", "hex-lo cell -> byte 5"
        )
        acol, arow = tf.invoke(f"{ORACLE}/ascii_cell", "5").split(",")
        assert_eq(tf.invoke(f"{ORACLE}/byte_at_cell", f"{acol},{arow}"), "5", "ascii cell -> byte 5")
        assert_eq(tf.invoke(f"{ORACLE}/byte_at_cell", "0,0"), "none", "offset column -> no byte")
        assert_eq(tf.invoke(f"{ORACLE}/byte_at_cell", "77,0"), "none", "gutter bar -> no byte")

        # --- real clicks: the router path resolves the byte and rings it ---
        # byte 0 (header magic 0x50 = 'P'), row 0.
        s0, c0, r0 = click_byte(tf, 0)
        assert_ringed(tf, s0, 0, c0, r0)
        assert_eq(tf.query(f"{ORACLE}/cursor_value"), "50", "byte 0 = 0x50")

        # byte 20 ('e' in the message), row 1 — proves the ring wraps rows.
        s20, c20, r20 = click_byte(tf, 20)
        assert_ringed(tf, s20, 20, c20, r20)
        assert_eq(r20, 1, "byte 20 is on row 1")

        # byte 127 (the last byte), row 7 — the far corner.
        s127, c127, r127 = click_byte(tf, 127)
        assert_ringed(tf, s127, 127, c127, r127)
        assert_eq(r127, 7, "byte 127 is on the last row")

        # --- click a non-byte cell (the offset column) -> deselect ---
        tf.click(at=(GRID_POS[0] + 2, GRID_POS[1] + CELL_H // 2))  # x in the offset field
        cleared = wait_snap(
            tf,
            lambda s: cursor(s).get("visible") is False,
            source="paint",
            viewport=WIN,
            desc="click on the offset column deselects",
        )
        assert_eq(cursor(cleared)["visible"], False, "offset-column click hid the cursor")
        assert_eq(tf.query(f"{ORACLE}/cursor_byte"), None, "cursor_byte cleared")

        # --- the AI-first write channel: intervene /external/cursor_byte ---
        tf.intervene(f"{ORACLE}/cursor_byte", 42)
        hc42, hr42 = hex_cell(tf, 42)
        witness = wait_snap(
            tf,
            lambda s: cursor(s).get("visible") is True and cursor(s).get("col") == hc42,
            source="paint",
            viewport=WIN,
            desc="intervene rings byte 42 with no pixel",
        )
        assert_eq(tf.query(f"{ORACLE}/cursor_byte"), 42, "intervene selected byte 42")
        assert_ringed(tf, witness, 42, hc42, hr42)

        # intervene null deselects.
        tf.intervene(f"{ORACLE}/cursor_byte", None)
        gone = wait_snap(
            tf,
            lambda s: cursor(s).get("visible") is False,
            source="paint",
            viewport=WIN,
            desc="intervene null deselects",
        )
        assert_eq(cursor(gone)["visible"], False, "null intervene hid the cursor")
        assert_eq(tf.query(f"{ORACLE}/cursor_value"), None, "cursor_value null after clear")


if __name__ == "__main__":
    sys.exit(run_demo("r1401_hex_cursor", body))
