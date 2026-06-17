#!/usr/bin/env python3
"""R973 §5.41 — cell-native `TextGrid` content data model (S5 slice 1).

The first S5 data-model slice: the terminal colour model (`TermColor` =
default / indexed / truecolor) + the per-grid `Palette` (indexed -> rgb
SSOT, resolved at paint time) + the `GridBuffer` cell projection. The
`hello-textgrid` window grows a third grid, `htg_content` (16x4 cells at
the 8x16 baseline), whose cells exercise every colour form.

The proof is pure DATA over RPC (glyph paint is still a follow-up round,
so the window renders only its surface). `scene/snapshot` now reports a
`grid_rows` projection for a populated grid: one entry per row, each with
the row `text` (clusters joined) and its style `runs` (maximal spans of
one fg/bg). Each colour reports BOTH its stored form (`kind` / `index`)
AND the palette-resolved `rgb` hex — so an AI client reads the producer's
semantic intent and the concrete colour a painter would draw, with NO OCR
(the §2 #7 scene-as-data thesis + R969 "resolve at paint time"):

  * the ANSI 16-colour bar: 16 indexed backgrounds resolve through the
    palette (`Indexed(1)` -> `#cd0000`, `Indexed(15)` -> `#ffffff`, ...);
  * a default-coloured row collapses to ONE run (run-length style);
  * truecolor foregrounds resolve verbatim, palette-independent;
  * the colour cube (`Indexed(196)` -> `#ff0000`) and grayscale ramp
    (`Indexed(232)` -> `#080808`) use the xterm formulas;
  * the two scaffold grids stay empty projections (`grid_rows == []`),
    so the R972 geometry demo is regression-free.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r973_textgrid_cells.py
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
CONTENT_TAG = "htg_content"

# The xterm default palette resolutions the snapshot must report.
DEFAULT_FG = "#e5e5e5"  # ANSI 7 (light grey)
DEFAULT_BG = "#000000"  # ANSI 0 (black)
# Indexed -> rgb hex (ANSI base, colour cube, grayscale ramp).
ANSI = {0: "#000000", 1: "#cd0000", 4: "#0000ee", 9: "#ff0000", 15: "#ffffff"}
CUBE_196 = "#ff0000"
GRAY_232 = "#080808"


def color(run: dict, slot: str) -> dict:
    """The fg/bg colour object of a style run."""
    return run[slot]


def assert_color(run: dict, slot: str, kind: str, index, rgb: str, ctx: str) -> None:
    c = color(run, slot)
    assert_eq(c["kind"], kind, f"{ctx} {slot}.kind")
    assert_eq(c["index"], index, f"{ctx} {slot}.index")
    assert_eq(c["rgb"], rgb, f"{ctx} {slot}.rgb")


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll the paint snapshot until the layout pass
        # has resolved the content grid (cols == 16) AND its projection is
        # present (4 rows). No fixed sleep — observed-state polling.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, CONTENT_TAG) or {}).get("cols") == 16
            and len((find_by_tag(s, CONTENT_TAG) or {}).get("grid_rows", [])) == 4,
            source="paint",
            viewport=WIN,
            desc=f"{CONTENT_TAG} projection resolved",
        )

        grid = find_by_tag(snap, CONTENT_TAG)
        assert grid is not None, "content grid present in paint scene"
        assert_eq(grid["type"], "TextGrid", "content type")
        assert_eq((grid["cols"], grid["rows"]), (16, 4), "content dims 16x4")
        # R974.1 — the projection's own dims match the winsize (steady state).
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (16, 4), "content buffer 16x4")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 4, "grid_rows has one entry per row")

        # --- Row 0: ANSI 16-colour bar (indexed backgrounds) ---
        bar = rows[0]
        assert_eq(bar["text"], " " * 16, "row0 text = 16 spaces")
        assert_eq(len(bar["runs"]), 16, "row0 has 16 distinct-bg runs")
        for i, run in enumerate(bar["runs"]):
            assert_eq(run["start"], i, f"row0 run{i} start")
            assert_eq(run["len"], 1, f"row0 run{i} len")
            # Foreground is default everywhere on the bar.
            assert_color(run, "fg", "default", None, DEFAULT_FG, f"row0 run{i}")
            # Background is the indexed palette entry for this column.
            assert_eq(run["bg"]["kind"], "indexed", f"row0 run{i} bg indexed")
            assert_eq(run["bg"]["index"], i, f"row0 run{i} bg index")
        # Spot-check the palette resolution against the xterm defaults.
        for idx, hexv in ANSI.items():
            assert_eq(bar["runs"][idx]["bg"]["rgb"], hexv, f"ANSI {idx} resolves {hexv}")

        # --- Row 1: default-coloured label collapses to ONE run ---
        label = rows[1]
        assert label["text"].startswith("Default"), "row1 text starts 'Default'"
        assert_eq(len(label["text"]), 16, "row1 text padded to width")
        assert_eq(len(label["runs"]), 1, "row1 is a single style run")
        run = label["runs"][0]
        assert_eq((run["start"], run["len"]), (0, 16), "row1 run spans whole row")
        assert_color(run, "fg", "default", None, DEFAULT_FG, "row1")
        assert_color(run, "bg", "default", None, DEFAULT_BG, "row1")

        # --- Row 2: truecolor foregrounds resolve verbatim ---
        tc = rows[2]
        assert tc["text"].startswith("RGB"), "row2 text starts 'RGB'"
        assert_eq(len(tc["runs"]), 4, "row2 has R/G/B + trailing default")
        assert_color(tc["runs"][0], "fg", "rgb", None, "#ff0000", "row2 R")
        assert_color(tc["runs"][1], "fg", "rgb", None, "#00ff00", "row2 G")
        assert_color(tc["runs"][2], "fg", "rgb", None, "#0000ff", "row2 B")
        # The trailing run is default-coloured blanks.
        tail = tc["runs"][3]
        assert_eq((tail["start"], tail["len"]), (3, 13), "row2 tail run")
        assert_color(tail, "fg", "default", None, DEFAULT_FG, "row2 tail")

        # --- Row 3: indexed cube + grayscale + ANSI on-colour ---
        mixed = rows[3]
        assert mixed["text"].startswith("#gy"), "row3 text starts '#gy'"
        assert_eq(len(mixed["runs"]), 4, "row3 has 3 distinct cells + tail")
        # white-on-blue ANSI.
        assert_color(mixed["runs"][0], "fg", "indexed", 15, ANSI[15], "row3 #")
        assert_color(mixed["runs"][0], "bg", "indexed", 4, ANSI[4], "row3 #")
        # colour-cube red foreground.
        assert_color(mixed["runs"][1], "fg", "indexed", 196, CUBE_196, "row3 g")
        assert_color(mixed["runs"][1], "bg", "default", None, DEFAULT_BG, "row3 g")
        # grayscale-ramp darkest foreground.
        assert_color(mixed["runs"][2], "fg", "indexed", 232, GRAY_232, "row3 y")

        # --- The scaffold grids stay empty projections (R972 regression-free) ---
        for tag in ("htg_default", "htg_measured"):
            g = find_by_tag(snap, tag)
            assert g is not None, f"{tag} present"
            assert_eq(g["grid_rows"], [], f"{tag} carries no cell projection")
            # R974.1 — a geometry-only grid: winsize requested, 0x0 received.
            assert_eq((g["buffer_cols"], g["buffer_rows"]), (0, 0), f"{tag} empty buffer")
            assert g["cols"] > 0 and g["rows"] > 0, f"{tag} has a derived winsize"


def main() -> int:
    return run_demo("r973_textgrid_cells", body)


if __name__ == "__main__":
    sys.exit(main())
