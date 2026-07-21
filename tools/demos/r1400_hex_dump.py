#!/usr/bin/env python3
"""R1400 §5.41 — a hex/byte-dump viewer over `Scene::TextGrid` with a
byte-range brush that highlights across the hex column AND the ascii gutter.

A fixed byte buffer renders as the classic three-region dump — an offset
column, the 16 bytes as `8 + 8` hex pairs, and an ascii gutter (each
non-printable byte shown as `.`) — in one `Scene::TextGrid`. A byte appears in
TWO disjoint cell regions (its two hex digits and its one ascii char), so
selecting a field must light BOTH regions together. A primary `Brush` over the
byte offsets `[0, N]` selects a contiguous byte window `[lo, hi)`; the view
paints a distinct (`rgb`) background on every selected byte's two hex cells and
its one ascii cell, leaving the rest `default`.

The proof is pure DATA over RPC (no OCR): `scene/snapshot` reports the
highlight in `grid_rows` (a selected cell's run carries an `rgb` `bg`), and the
primary `HexDumpOracle` (`/external/*`) exposes the layout so the byte↔cell
mapping is read, not guessed. Two invariants pin the linked highlight:

  * global — the count of `rgb`-background cells is exactly `3 * (hi - lo)`
    (two hex cells + one ascii cell per selected byte), and nothing else is
    tinted;
  * per byte — for a sampled byte the oracle's `hex_cell` and `ascii_cell`
    BOTH resolve to `rgb` (a selected byte, not its neighbour) or BOTH to
    `default` (an unselected one).

Run from the workspace root:
    cargo build -p hello-hex-dump --release
    python3 tools/demos/r1400_hex_dump.py
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
BRUSH = "/hex_brush/external"

BYTE_COUNT = 128
BYTES_PER_ROW = 16
TOTAL_COLS = 78
ROWS = 8


# --- snapshot helpers ------------------------------------------------------


def grid_rows(snap) -> list:
    grid = find_by_tag(snap, GRID)
    assert grid is not None, "the hex-dump grid is in the paint scene"
    return grid["grid_rows"]


def run_at(row: dict, col: int) -> dict:
    """The style run covering `col` in a row (`grid_rows` is RLE by style)."""
    for run in row["runs"]:
        if run["start"] <= col < run["start"] + run["len"]:
            return run
    raise AssertionError(f"column {col} is covered by some run")


def bg_kind_at(row: dict, col: int) -> str:
    return run_at(row, col)["bg"]["kind"]


def rgb_bg_cells(snap) -> int:
    """Total cells whose background is an `rgb` fill — the highlight tint."""
    n = 0
    for row in grid_rows(snap):
        for run in row["runs"]:
            if run["bg"]["kind"] == "rgb":
                n += run["len"]
    return n


# --- oracle helpers --------------------------------------------------------


def cell_of(tf, which: str, b: int) -> tuple[int, int]:
    """`/external/{hex_cell,ascii_cell}` -> the byte's `(col, row)`."""
    col, row = tf.invoke(f"/external/{which}", str(b)).split(",")
    return int(col), int(row)


def byte_window(tf, low: float, high: float) -> tuple[int, int]:
    """`/external/byte_window` -> the `(lo, hi)` byte range for the fractions —
    the same SSOT the view highlights from."""
    lo, hi = tf.invoke("/external/byte_window", f"{low},{high}").split(",")
    return int(lo), int(hi)


def assert_byte_lit(tf, rows: list, b: int, lit: bool) -> None:
    """A byte is lit iff BOTH its hex pair AND its ascii cell carry the tint —
    the two-region linked highlight (or context in both when not lit)."""
    expect = "rgb" if lit else "default"
    hcol, hrow = cell_of(tf, "hex_cell", b)
    acol, arow = cell_of(tf, "ascii_cell", b)
    assert_eq(bg_kind_at(rows[hrow], hcol), expect, f"byte {b} hex-hi bg")
    assert_eq(bg_kind_at(rows[hrow], hcol + 1), expect, f"byte {b} hex-lo bg")
    assert_eq(bg_kind_at(rows[arow], acol), expect, f"byte {b} ascii bg")


def set_brush(tf, low: float, high: float) -> tuple[int, int, object]:
    """Drive the brush, wait until the paint's highlight settles to the oracle's
    byte window, and return `(lo, hi, snap)`."""
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
        grid = find_by_tag(s, GRID)
        if grid is None:
            return False
        return rgb_bg_cells(s) == 3 * (hi - lo)

    snap = wait_snap(
        tf,
        settled,
        source="paint",
        viewport=WIN,
        desc=f"brush {low:.3f}..{high:.3f} highlight settles",
    )
    return box["lo"], box["hi"], snap


def check_window(tf, low: float, high: float, sample: list[int]) -> None:
    """Drive a window and assert both invariants: the global rgb-cell count and
    the per-byte linked highlight for the sampled boundary bytes."""
    lo, hi, snap = set_brush(tf, low, high)
    rows = grid_rows(snap)
    # Global invariant: exactly 3 tinted cells (2 hex + 1 ascii) per byte.
    assert_eq(rgb_bg_cells(snap), 3 * (hi - lo), f"window {lo}..{hi} rgb-cell count")
    # Per-byte linked highlight for the sampled bytes.
    for b in sample:
        if 0 <= b < BYTE_COUNT:
            assert_byte_lit(tf, rows, b, lo <= b < hi)


def body() -> None:
    with RpcSubprocess("hello-hex-dump") as tf:
        # ZERO-FLAKE gate: poll until the grid's layout resolved (cols == 78)
        # and its 8-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == TOTAL_COLS
            and len((find_by_tag(s, GRID) or {}).get("grid_rows", [])) == ROWS,
            source="paint",
            viewport=WIN,
            desc="hex-dump grid projection resolved",
        )

        grid = find_by_tag(snap, GRID)
        assert grid is not None, "grid present in the paint scene"
        assert_eq((grid["cols"], grid["rows"]), (TOTAL_COLS, ROWS), "grid dims 78x8")

        # --- the layout oracle (primary external) ---
        assert_eq(tf.query("/external/byte_count"), BYTE_COUNT, "byte_count")
        assert_eq(tf.query("/external/bytes_per_row"), BYTES_PER_ROW, "bytes_per_row")
        assert_eq(tf.query("/external/row_count"), ROWS, "row_count")
        assert_eq(tf.query("/external/total_cols"), TOTAL_COLS, "total_cols")

        # byte -> cell mappings.
        assert_eq(cell_of(tf, "hex_cell", 0), (10, 0), "byte 0 hex cell")
        assert_eq(cell_of(tf, "ascii_cell", 0), (61, 0), "byte 0 ascii cell")
        assert_eq(cell_of(tf, "hex_cell", 8), (35, 0), "byte 8 hex cell (group gap)")
        assert_eq(cell_of(tf, "hex_cell", 16), (10, 1), "byte 16 hex cell (row 1)")
        assert_eq(cell_of(tf, "ascii_cell", 20), (65, 1), "byte 20 ascii cell (row 1)")
        # A byte past the buffer has no cell.
        assert_eq(tf.invoke("/external/hex_cell", "128"), "none", "byte 128 has no cell")
        # byte_window mirrors the view's SSOT.
        assert_eq(byte_window(tf, 0.0, 1.0), (0, BYTE_COUNT), "full-span byte window")
        assert_eq(byte_window(tf, 0.0, 0.5), (0, 64), "half byte window")

        rows = grid["grid_rows"]

        # --- the static dump structure ---
        # Each row's offset column is its start offset, 8 hex digits.
        for r in range(ROWS):
            assert_eq(rows[r]["text"][0:8], f"{r * BYTES_PER_ROW:08x}", f"row {r} offset")
        # Row 0 header: the magic byte 0 = 0x50 = "50" at its hex columns.
        assert_eq(rows[0]["text"][10:12], "50", "byte 0 hex = 50")
        # The ascii gutter of row 0 (cols 61..77), bracketed by the `|` bars.
        assert_eq(rows[0]["text"][60], "|", "gutter left bar")
        assert_eq(rows[0]["text"][77], "|", "gutter right bar")
        assert_eq(rows[0]["text"][61:77], "PIN....,payload:", "row 0 ascii gutter")
        # Non-printable byte 3 (0x01) shows a '.' in the gutter, hex still "01".
        assert_eq(rows[0]["text"][10 + 3 * 3 : 10 + 3 * 3 + 2], "01", "byte 3 hex = 01")

        # --- boot selection: the seeded field (bytes 8..24) is lit ---
        boot_low = tf.query(f"{BRUSH}/low")
        boot_high = tf.query(f"{BRUSH}/high")
        blo, bhi = byte_window(tf, boot_low, boot_high)
        assert_eq((blo, bhi), (8, 24), "boot window = the payload field 8..24")
        assert_eq(rgb_bg_cells(snap), 3 * (bhi - blo), "boot highlight count")
        # The seeded field is lit in both regions; byte 7 just before it is not.
        assert_byte_lit(tf, rows, 8, True)
        assert_byte_lit(tf, rows, 23, True)
        assert_byte_lit(tf, rows, 7, False)
        assert_byte_lit(tf, rows, 24, False)

        # --- drive: the length field (bytes 4..12, spans the group gap) ---
        check_window(tf, 4 / BYTE_COUNT, 12 / BYTE_COUNT, sample=[3, 4, 7, 8, 11, 12])

        # --- drive: a window crossing a ROW boundary (bytes 12..20) ---
        check_window(tf, 12 / BYTE_COUNT, 20 / BYTE_COUNT, sample=[11, 12, 15, 16, 19, 20])

        # --- drive: the whole buffer (every byte lit) ---
        lo, hi, full = set_brush(tf, 0.0, 1.0)
        assert_eq((lo, hi), (0, BYTE_COUNT), "full span selects every byte")
        assert_eq(rgb_bg_cells(full), 3 * BYTE_COUNT, "every byte tinted")
        assert_byte_lit(tf, grid_rows(full), 0, True)
        assert_byte_lit(tf, grid_rows(full), 127, True)

        # --- drive: a narrow interior window on the last row (bytes 116..124) ---
        check_window(tf, 116 / BYTE_COUNT, 124 / BYTE_COUNT, sample=[115, 116, 123, 124])


if __name__ == "__main__":
    sys.exit(run_demo("r1400_hex_dump", body))
