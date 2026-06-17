#!/usr/bin/env python3
"""R978 §5.41 — cell-native `TextGrid` damage tracking (S5 slice 6, last).

The final S5 data-model slice, completing the cell-native TextGrid model.
A streaming terminal (an AI agent watching a CLI emit output) needs to know
WHICH rows changed since it last looked, without re-reading the whole grid.
Each row carries a monotonic `generation` the producer bumps when it
rewrites the row (a real emulator tracks dirty lines — R969 "producer owns
authoritative state"). A client remembers the highest generation it has
read and, on its next `scene/snapshot`, re-reads only the rows whose
generation exceeds that high-water mark — an incremental update that
survives an arbitrary polling cadence (unlike a frame-reset dirty flag).
pinion stores and reports the producer's stamps; it does not diff buffers.

The `hello-textgrid` window grows an eighth grid, `htg_damage`: three
streamed output lines, the newest stamped highest (generations 10 / 20 /
30). Glyph paint is still a follow-up round — the proof is pure DATA.

  * each `grid_rows` entry now carries a `generation`;
  * a client at a given baseline computes its changed-set (the rows whose
    generation exceeds the baseline) and re-reads only those;
  * the `generation` field is additive — every other grid leaves its rows
    at generation 0, so R972-R977 demos are regression-free.

This is a snapshot-in-time of a streaming terminal; live cross-frame
mutation is the producer's (sprag's) domain — this slice ships the data
model that enables the incremental read.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r978_textgrid_damage.py
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
DAMAGE_TAG = "htg_damage"


def changed_since(rows: list, baseline: int) -> list:
    """The row indices a client re-reads given its last-seen high-water
    mark — exactly the rows whose damage generation exceeds the baseline."""
    return [i for i, r in enumerate(rows) if r["generation"] > baseline]


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the damage grid's layout has resolved
        # (cols == 8) AND its 3-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, DAMAGE_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, DAMAGE_TAG) or {}).get("grid_rows", [])) == 3,
            source="paint",
            viewport=WIN,
            desc=f"{DAMAGE_TAG} projection resolved",
        )

        grid = find_by_tag(snap, DAMAGE_TAG)
        assert grid is not None, "damage grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 3), "damage dims 8x3")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 3, "damage grid has one entry per row")

        # --- Per-row monotonic damage generations ---
        gens = [r["generation"] for r in rows]
        assert_eq(gens, [10, 20, 30], "rows stamped with increasing generations")
        # Each row's text is its streamed output line.
        assert_eq(rows[0]["text"], "line 1  ", "row0 = first output line")
        assert_eq(rows[2]["text"], "line 3  ", "row2 = newest output line")
        # The newest line carries the highest generation.
        assert gens[2] == max(gens), "newest row has the highest generation"

        # --- The incremental-read model (what a streaming client does) ---
        # A client whose high-water mark is 15 re-reads only rows 1 and 2.
        assert_eq(changed_since(rows, 15), [1, 2], "baseline 15 -> rows 1,2")
        # Advancing the baseline to 25 narrows the changed-set to row 2.
        assert_eq(changed_since(rows, 25), [2], "baseline 25 -> row 2")
        # At the high-water mark 30 nothing is newer — no re-read needed.
        assert_eq(changed_since(rows, 30), [], "baseline 30 -> nothing newer")
        # A fresh client (baseline 0) reads every row.
        assert_eq(changed_since(rows, 0), [0, 1, 2], "baseline 0 -> all rows")

        # --- Regression: every other grid leaves its rows at generation 0 ---
        for tag in ("htg_content", "htg_attrs", "htg_cursor", "htg_wide", "htg_alt"):
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            for ri, row in enumerate(other["grid_rows"]):
                assert_eq(row["generation"], 0, f"{tag} row{ri} generation 0")

        # --- Regression: R977 screen / R976 width fields untouched ---
        assert_eq(grid["screen"], "main", "damage grid is the main screen")
        assert_eq(rows[0]["runs"][0]["width"], "narrow", "damage rows are narrow")


def main() -> int:
    return run_demo("r978_textgrid_damage", body)


if __name__ == "__main__":
    sys.exit(main())
