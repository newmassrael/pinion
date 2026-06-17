#!/usr/bin/env python3
"""R976 §5.41 — cell-native `TextGrid` wide-char trailer (S5 slice 4).

The fourth S5 data-model slice: wide clusters. A CJK ideograph or a
fullwidth Latin form occupies TWO terminal columns — a head cell plus a
continuation **trailer** that carries no independent glyph (`CellWidth` =
Narrow / Wide / Trailer). The `hello-textgrid` window grows a sixth grid,
`htg_wide` (8x2 cells), isolated from R973/R974/R975 so their demos stay
regression-free.

The proof is pure DATA over RPC (glyph paint is still a follow-up round).
Each `scene/snapshot` style run now carries a `width` role, and the run
key is `(fg, bg, attrs, width)` so a wide head and its trailer are each
their own run. The width is the PRODUCER's determination — pinion stores
it, it does not compute width (R969 "producer owns authoritative state"):

  * row 0 is two wide CJK ideographs (世界): 4 columns, each a head (wide)
    + a trailer; the trailer shares the head's colour and contributes no
    glyph, so the row text holds each ideograph once (2 glyphs, 4 cols);
  * row 1 mixes narrow + wide: A (narrow), Ａ (wide fullwidth, head +
    trailer), B (narrow) — proving glyph-to-column mapping where the two
    widths interleave;
  * the `width` field is additive — every cell in the other grids is
    `narrow`, so R973/R974/R975 demos are regression-free.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r976_textgrid_wide.py
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

WIN = (680, 840)
WIDE_TAG = "htg_wide"

# The wide forms (kept as escapes for clarity; Python source is UTF-8 safe).
SHI = "世"   # 世  East Asian Wide
JIE = "界"   # 界  East Asian Wide
FW_A = "Ａ"  # Ａ  Fullwidth Latin A


def widths(row: dict) -> list:
    """The width role of each run in column order."""
    return [run["width"] for run in row["runs"]]


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the wide grid's layout has resolved
        # (cols == 8) AND its 2-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, WIDE_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, WIDE_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{WIDE_TAG} projection resolved",
        )

        grid = find_by_tag(snap, WIDE_TAG)
        assert grid is not None, "wide grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 2), "wide dims 8x2")
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (8, 2), "wide buffer 8x2")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "wide grid_rows has one entry per row")

        # --- Row 0: two wide CJK ideographs (head + trailer) ---
        r0 = rows[0]
        # Each ideograph appears once even though it spans two columns.
        assert_eq(r0["text"], SHI + JIE + "    ", "row0 = 世界 once + 4 blanks")
        assert_eq(len(r0["text"]), 6, "row0 text = 2 glyphs + 4 blanks (not 8)")
        assert_eq(len(r0["runs"]), 5, "row0: wide/trailer x2 + blank tail")
        assert_eq(widths(r0), ["wide", "trailer", "wide", "trailer", "narrow"], "row0 widths")
        # The head keeps its colour; the trailer shares it (bg paints across).
        assert_eq(r0["runs"][0]["fg"]["index"], 1, "row0 世 head fg index 1")
        assert_eq(r0["runs"][1]["fg"]["index"], 1, "row0 trailer shares head fg")
        assert_eq(r0["runs"][2]["fg"]["index"], 2, "row0 界 head fg index 2")
        assert_eq(r0["runs"][3]["fg"]["index"], 2, "row0 trailer shares head fg")
        # The two wide heads occupy 4 columns; the tail is a narrow blank run.
        assert_eq([r["start"] for r in r0["runs"][:4]], [0, 1, 2, 3], "row0 head/trailer columns")
        assert_eq((r0["runs"][4]["start"], r0["runs"][4]["len"]), (4, 4), "row0 blank tail 4..8")
        wide_heads = [r for r in r0["runs"] if r["width"] == "wide"]
        assert_eq(len(wide_heads), 2, "row0 has exactly two wide heads")

        # --- Row 1: narrow + wide interleaved (A Ａ B) ---
        r1 = rows[1]
        assert_eq(r1["text"], "A" + FW_A + "B" + "    ", "row1 = A Ａ B once + 4 blanks")
        assert_eq(widths(r1), ["narrow", "wide", "trailer", "narrow"], "row1 widths")
        assert_eq(r1["runs"][0]["width"], "narrow", "row1 A is narrow")
        assert_eq((r1["runs"][1]["start"], r1["runs"][1]["len"]), (1, 1), "row1 Ａ head col1")
        assert_eq(r1["runs"][1]["fg"]["index"], 4, "row1 Ａ head fg index 4")
        assert_eq(r1["runs"][2]["fg"]["index"], 4, "row1 trailer shares Ａ fg")
        # B + 4 blanks share (narrow, default) so they coalesce into one run.
        assert_eq((r1["runs"][3]["start"], r1["runs"][3]["len"]), (3, 5), "row1 B + blank tail")

        # --- Regression: every other grid is all-narrow ---
        for tag in ("htg_content", "htg_attrs", "htg_cursor"):
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            for ri, row in enumerate(other["grid_rows"]):
                for run in row["runs"]:
                    assert_eq(run["width"], "narrow", f"{tag} row{ri} run is narrow")

        # --- Regression: R973 colours / R974 attrs untouched (additive) ---
        content = find_by_tag(snap, "htg_content")
        bar0 = content["grid_rows"][0]["runs"][0]
        assert_eq(bar0["bg"]["index"], 0, "htg_content bar[0] bg still index 0")
        assert_eq(bar0["attrs"]["bold"], False, "htg_content attrs still off")


def main() -> int:
    return run_demo("r976_textgrid_wide", body)


if __name__ == "__main__":
    sys.exit(main())
