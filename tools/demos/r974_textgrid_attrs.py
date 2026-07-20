#!/usr/bin/env python3
"""R974 §5.41 — cell-native `TextGrid` SGR attributes (S5 slice 2).

The second S5 data-model slice: per-cell SGR display attributes
(`CellAttrs` = bold / dim / italic / underline / blink / reverse / hidden
/ strikethrough). The `hello-textgrid` window grows a fourth grid,
`htg_attrs` (8x2 cells), isolated from R973's `htg_content` so the colour
demo stays regression-free.

The proof is pure DATA over RPC (glyph paint is still a follow-up round).
Each `scene/snapshot` style run now carries an `attrs` object of named
SGR booleans, and the run key is `(fg, bg, attrs)` so a styling change
starts a new run. `reverse` is reported as the cell's STORED flag with
its stored (unswapped) colours — a renderer applies the fg/bg swap at
paint time (the §2 #7 scene-as-data thesis + R969 "resolve at paint
time"):

  * row 0 shows each SGR flag in isolation (one flag per cell -> 8 runs);
  * row 1 shows combinations (bold+italic, underline+strikethrough) and a
    reverse cell whose Indexed(1)/Indexed(15) colours are unswapped;
  * R973's colour grid carries `attrs` all-false on every run (the new
    field is additive — the colour demo is regression-free).

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r974_textgrid_attrs.py
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
ATTRS_TAG = "htg_attrs"

# The boolean SGR flags. R1399 lifted `underline` out of the bool set: it
# is now the SGR 4:x `UnderlineStyle` string axis ("none" .. "dashed"), so
# the all-off default carries "none" and `only("underline")` means "single".
BOOL_FLAGS = ["bold", "dim", "italic", "blink", "reverse", "hidden", "strikethrough"]
# Row 0 sets one attribute per cell, in this declaration order (underline in
# slot 3).
ROW0_ORDER = ["bold", "dim", "italic", "underline", "blink", "reverse", "hidden", "strikethrough"]
ALL_OFF = {**{f: False for f in BOOL_FLAGS}, "underline": "none"}


def only(attr: str) -> dict:
    """The attrs object with exactly `attr` set (underline -> the string 'single')."""
    d = dict(ALL_OFF)
    if attr == "underline":
        d["underline"] = "single"
    else:
        d[attr] = True
    return d


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the attrs grid's layout has resolved
        # (cols == 8) AND its 2-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, ATTRS_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, ATTRS_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{ATTRS_TAG} projection resolved",
        )

        grid = find_by_tag(snap, ATTRS_TAG)
        assert grid is not None, "attrs grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 2), "attrs dims 8x2")
        # R974.1 — projection dims match the winsize (steady state).
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (8, 2), "attrs buffer 8x2")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "attrs grid_rows has one entry per row")

        # --- Row 0: one SGR flag per cell ---
        r0 = rows[0]
        assert_eq(r0["text"], "BDIUKRHS", "row0 text = one glyph per flag")
        assert_eq(len(r0["runs"]), 8, "row0 has 8 single-flag runs")
        for i, flag in enumerate(ROW0_ORDER):
            run = r0["runs"][i]
            assert_eq(run["start"], i, f"row0 {flag} run start")
            assert_eq(run["len"], 1, f"row0 {flag} run len")
            # Exactly this one flag is set, all others off.
            assert_eq(run["attrs"], only(flag), f"row0 cell {i} attrs = only {flag}")
            # R1399 — the additive underline colour is null unless SGR 58 set.
            assert_eq(run["underline_color"], None, f"row0 {flag} underline_color null")
            # Attributes do not disturb the (default) colours.
            assert_eq(run["fg"]["kind"], "default", f"row0 {flag} fg default")
            assert_eq(run["fg"]["rgb"], "#e5e5e5", f"row0 {flag} fg rgb")

        # --- Row 1: combinations + reverse with stored (unswapped) colours ---
        r1 = rows[1]
        assert r1["text"].startswith("abc"), "row1 text starts 'abc'"
        assert_eq(len(r1["runs"]), 4, "row1 has 3 styled cells + blank tail")

        combo = r1["runs"][0]
        assert combo["attrs"]["bold"] and combo["attrs"]["italic"], "row1 cell0 bold+italic"
        assert_eq(combo["attrs"]["underline"], "none", "row1 cell0 no underline")

        deco = r1["runs"][1]
        assert_eq(deco["attrs"]["underline"], "single", "row1 cell1 underlined")
        assert deco["attrs"]["strikethrough"], "row1 cell1 struck through"

        rev = r1["runs"][2]
        assert rev["attrs"]["reverse"], "row1 cell2 reverse set"
        # The stored colours are reported unswapped; the renderer swaps at paint.
        assert_eq(rev["fg"]["kind"], "indexed", "row1 cell2 fg kind")
        assert_eq(rev["fg"]["index"], 1, "row1 cell2 fg index (stored, not swapped)")
        assert_eq(rev["fg"]["rgb"], "#cd0000", "row1 cell2 fg rgb")
        assert_eq(rev["bg"]["index"], 15, "row1 cell2 bg index (stored, not swapped)")
        assert_eq(rev["bg"]["rgb"], "#ffffff", "row1 cell2 bg rgb")

        tail = r1["runs"][3]
        assert_eq((tail["start"], tail["len"]), (3, 5), "row1 blank tail run")
        assert_eq(tail["attrs"], ALL_OFF, "row1 tail attrs all off")

        # --- Regression: R973 colour grid runs carry attrs all-false ---
        content = find_by_tag(snap, "htg_content")
        assert content is not None, "htg_content still present"
        bar0 = content["grid_rows"][0]["runs"][0]
        assert_eq(bar0["attrs"], ALL_OFF, "htg_content bar run has attrs all off")
        # And the new attrs field is additive — colours are unchanged.
        assert_eq(bar0["bg"]["index"], 0, "htg_content bar[0] bg still index 0")


def main() -> int:
    return run_demo("r974_textgrid_attrs", body)


if __name__ == "__main__":
    sys.exit(main())
