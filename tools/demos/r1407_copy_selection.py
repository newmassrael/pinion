#!/usr/bin/env python3
"""R1407 §5.41 §5.22 — copy the hex-dump's byte selection to the clipboard.

Ctrl/Cmd+C copies the R1402 byte selection as a lowercase hex string, through the
lifted `pinion_core::external::selection_copy_payload` chord handler — the third
consumer of the copy chord after hello-cell-select (R1222) and hello-data-grid
(R1372), each of which serialises a DIFFERENT selection (cell TSV vs hex bytes)
through the SAME gate + query.

The OS clipboard write itself is HW-gated (driving a real Ctrl+C on a live
display would clobber / race the user's clipboard), so this demo verifies the
copy's AI-observable half: the **copyable payload** — the exact string a Ctrl+C
would write — is `query selection_hex`, and it is (a) identical whichever way the
selection was made (a real drag, an `invoke select_range`, an `intervene
cursor_byte`), and (b) always equal to the hex digits the grid actually paints at
the selected cells. Readout and paint can never diverge, so an AI client reads
exactly what a human's Ctrl+C copies — with no pixel.

Run from the workspace root:
    cargo build -p hello-hex-dump --release
    python3 tools/demos/r1407_copy_selection.py
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


def hex_cell(tf, b: int) -> tuple[int, int]:
    col, row = tf.invoke(f"{ORACLE}/hex_cell", str(b)).split(",")
    return int(col), int(row)


def byte_xy(tf, b: int) -> tuple[int, int]:
    """The window pixel at byte `b`'s hex high-nibble cell centre."""
    col, row = hex_cell(tf, b)
    return (GRID_POS[0] + col * CELL_W + CELL_W // 2, GRID_POS[1] + row * CELL_H + CELL_H // 2)


def cursor(snap) -> dict:
    grid = find_by_tag(snap, GRID)
    assert grid is not None, "the hex-dump grid is in the paint scene"
    return grid["cursor"]


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


def painted_hex(tf, snap, start: int, end: int) -> str:
    """The hex digits the grid actually PAINTS for bytes [start, end) — the
    ground truth a Ctrl+C must match (read from the grid's `grid_rows` text)."""
    rows = find_by_tag(snap, GRID)["grid_rows"]
    out = ""
    for b in range(start, end):
        hc, hr = hex_cell(tf, b)
        out += rows[hr]["text"][hc : hc + 2]
    return out


def copyable(tf) -> str | None:
    """The payload a Ctrl+C would write — the copy chord's AI-first peer."""
    return tf.query(f"{ORACLE}/selection_hex")


def assert_copy(tf, snap, start: int, end: int, expected: str) -> None:
    """The copy contract: the copyable payload equals the expected hex AND the
    painted hex, is lowercase, and is exactly 2 chars per selected byte."""
    payload = copyable(tf)
    assert_eq(payload, expected, f"copyable [{start},{end}) == {expected}")
    assert_eq(payload, painted_hex(tf, snap, start, end), "copyable == painted hex")
    assert payload is not None
    assert_eq(len(payload), 2 * (end - start), "two hex digits per byte")
    assert payload == payload.lower(), "copyable payload is lowercase hex"
    assert all(c in "0123456789abcdef" for c in payload), "copyable is valid hex"


def body() -> None:
    with RpcSubprocess("hello-hex-dump") as tf:
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == 78,
            source="paint",
            viewport=WIN,
            desc="hex-dump grid resolved",
        )

        # --- boot: nothing selected -> nothing to copy ---
        assert_eq(copyable(tf), None, "boot: no selection => no copyable payload")
        assert_eq(tf.query(f"{ORACLE}/selection_len"), None, "boot: no length")

        # --- the header's 4-byte length field [4, 8), via the no-pixel invoke ---
        assert_eq(tf.invoke(f"{ORACLE}/select_range", "4,8"), "4,8", "select_range [4,8)")
        s1 = wait_focus(tf, 7, "length field selected")
        assert_copy(tf, s1, 4, 8, "0000002c")

        # --- the SAME bytes selected by a REAL drag: the copyable payload is
        #     method-independent (a copy is of the SELECTION, not the gesture) ---
        tf.invoke(f"{ORACLE}/select_range", "none")
        tf.drag(from_at=byte_xy(tf, 4), to_at=byte_xy(tf, 7))
        s2 = wait_focus(tf, 7, "length field re-selected by drag")
        assert_copy(tf, s2, 4, 8, "0000002c")
        assert_eq(copyable(tf), "0000002c", "drag copies the same bytes as the invoke")

        # --- the "PIN\x01" magic [0, 4): includes a non-printable byte, which a
        #     hex copy still renders as its two digits (unlike the ascii gutter) ---
        tf.invoke(f"{ORACLE}/select_range", "0,4")
        s3 = wait_focus(tf, 3, "magic bytes selected")
        assert_copy(tf, s3, 0, 4, "50494e01")

        # --- a selection that SPANS two dump rows [14, 18): the copyable hex
        #     concatenates in byte order across the row boundary ---
        tf.invoke(f"{ORACLE}/select_range", "14,18")
        s4 = wait_focus(tf, 17, "cross-row selection")
        assert_copy(tf, s4, 14, 18, painted_hex(tf, s4, 14, 18))
        assert_eq(tf.query(f"{ORACLE}/selection_len"), 4, "spans four bytes")

        # --- a collapsed single-byte selection via the no-pixel intervene ---
        tf.intervene(f"{ORACLE}/cursor_byte", 0)
        s5 = wait_focus(tf, 0, "collapsed to byte 0")
        assert_copy(tf, s5, 0, 1, "50")

        # --- clearing the selection empties the copyable payload again ---
        tf.invoke(f"{ORACLE}/select_range", "none")
        gone = wait_snap(
            tf,
            lambda s: cursor(s).get("visible") is False,
            source="paint",
            viewport=WIN,
            desc="selection cleared",
        )
        assert_eq(cursor(gone)["visible"], False, "cleared: cursor hidden")
        assert_eq(copyable(tf), None, "cleared: nothing to copy")


if __name__ == "__main__":
    sys.exit(run_demo("r1407_copy_selection", body))
