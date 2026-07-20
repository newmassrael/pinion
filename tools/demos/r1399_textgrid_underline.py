#!/usr/bin/env python3
"""R1399 §5.41 — cell-native `TextGrid` underline style axis + SGR-58 colour.

The underline is no longer a bool: `CellAttrs.underline` is the ECMA-48 SGR
4:x `UnderlineStyle` axis (none / single / double / curly / dotted /
dashed), and `TermCell.underline_color` (SGR 58 set / 59 reset) carries an
explicit underline colour orthogonal to the glyph colour. The forcing case
is an editor's LSP diagnostics — a red-curly error vs a blue-dotted
spellcheck — which flatten to one indistinguishable rule without this.

The proof is pure DATA over RPC (the pixel witness is the #[ignore]d
`r1399_text_grid_paints_underline_styles_and_color` shell test). Each
`scene/snapshot` style run now reports `attrs.underline` as a
self-describing string discriminator (mirroring the cursor `shape`) and a
run-level `underline_color` object (`{kind,index,rgb}` or `null`), resolved
through the palette exactly like `fg` / `bg`:

  * `htg_underline` row 0 shows each style in isolation (one per cell), all
    with the default (SGR-59) underline colour `null`;
  * row 1 is the coloured-diagnostic case — a truecolor red curly, a
    palette-indexed yellow curly, a blue dotted, and a green single — each
    carrying an explicit `underline_color` while the glyph stays default;
  * regression: the R974 `htg_attrs` grid's underline cell now reports the
    string "single" (was the bool `true`) and every other attr cell "none".

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1399_textgrid_underline.py
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
UL_TAG = "htg_underline"
ATTRS_TAG = "htg_attrs"


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the underline grid's layout has resolved
        # (cols == 6) AND its 2-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, UL_TAG) or {}).get("cols") == 6
            and len((find_by_tag(s, UL_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{UL_TAG} projection resolved",
        )

        grid = find_by_tag(snap, UL_TAG)
        assert grid is not None, "underline grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (6, 2), "underline dims 6x2")
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (6, 2), "buffer 6x2")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "underline grid_rows has one entry per row")

        # --- Row 0: one UnderlineStyle per cell, default (null) colour ---
        r0 = rows[0]
        assert_eq(r0["text"], "sdcoan", "row0 text = one glyph per style")
        styles = ["single", "double", "curly", "dotted", "dashed", "none"]
        assert_eq(len(r0["runs"]), 6, "row0 has 6 single-style runs")
        for i, style in enumerate(styles):
            run = r0["runs"][i]
            assert_eq(run["start"], i, f"row0 {style} run start")
            assert_eq(run["len"], 1, f"row0 {style} run len")
            # The underline STYLE is a self-describing string in attrs.
            assert_eq(run["attrs"]["underline"], style, f"row0 cell {i} underline={style}")
            # No explicit SGR-58 colour: the default tracks the foreground.
            assert_eq(run["underline_color"], None, f"row0 {style} underline_color null")
            # Style does not disturb the (default) glyph colours.
            assert_eq(run["fg"]["kind"], "default", f"row0 {style} fg default")
        # The other SGR flags are unaffected by the underline axis change.
        for name in ("bold", "italic", "strikethrough", "reverse"):
            assert_eq(r0["runs"][0]["attrs"][name], False, f"row0 single {name} off")

        # --- Row 1: coloured diagnostics (explicit SGR-58 underline colour) ---
        r1 = rows[1]
        assert r1["text"].startswith("EWzg"), "row1 text starts 'EWzg'"
        # 4 coloured cells + a blank tail = 5 runs.
        assert_eq(len(r1["runs"]), 5, "row1 has 4 coloured cells + blank tail")

        # (0,1) red-curly error — truecolor underline colour, curly style.
        err = r1["runs"][0]
        assert_eq(err["attrs"]["underline"], "curly", "row1 error is curly")
        assert err["underline_color"] is not None, "row1 error has an underline colour"
        assert_eq(err["underline_color"]["kind"], "rgb", "error underline colour is truecolor")
        assert_eq(err["underline_color"]["rgb"], "#ff0000", "error underline colour is red")
        # The SGR-58 colour is independent of the glyph colour.
        assert_eq(err["fg"]["kind"], "default", "error glyph colour stays default")

        # (1,1) yellow-curly warning — same curly style, DIFFERENT colour, so a
        # new run (underline_color is part of the run key).
        warn = r1["runs"][1]
        assert_eq(warn["attrs"]["underline"], "curly", "row1 warning is curly")
        assert_eq(warn["underline_color"]["kind"], "indexed", "warning colour is palette-indexed")
        assert_eq(warn["underline_color"]["index"], 11, "warning colour index 11 (bright yellow)")
        assert_eq(warn["underline_color"]["rgb"], "#ffff00", "warning colour resolves yellow")

        # (2,1) blue-dotted spellcheck.
        spell = r1["runs"][2]
        assert_eq(spell["attrs"]["underline"], "dotted", "row1 spellcheck is dotted")
        assert_eq(spell["underline_color"]["index"], 4, "spellcheck colour index 4 (blue)")
        assert_eq(spell["underline_color"]["rgb"], "#0000ee", "spellcheck colour resolves blue")

        # (3,1) green single.
        green = r1["runs"][3]
        assert_eq(green["attrs"]["underline"], "single", "row1 green is single")
        assert_eq(green["underline_color"]["index"], 2, "green colour index 2")
        assert_eq(green["underline_color"]["rgb"], "#00cd00", "green colour resolves green")

        # The blank tail: no underline, no colour.
        tail = r1["runs"][4]
        assert_eq((tail["start"], tail["len"]), (4, 2), "row1 blank tail run")
        assert_eq(tail["attrs"]["underline"], "none", "row1 tail underline none")
        assert_eq(tail["underline_color"], None, "row1 tail underline_color null")

        # --- Regression: the R974 attrs grid's underline is now a string ---
        attrs = find_by_tag(snap, ATTRS_TAG)
        assert attrs is not None, "htg_attrs still present"
        a0 = attrs["grid_rows"][0]["runs"]
        # Row 0 declares flags in order B D I U K R H S — the U (index 3) cell.
        assert_eq(a0[3]["attrs"]["underline"], "single", "attrs 'U' cell now reports 'single'")
        assert_eq(a0[0]["attrs"]["underline"], "none", "attrs 'B' cell reports 'none'")
        # And the additive colour field is present + null on a non-coloured run.
        assert_eq(a0[0]["underline_color"], None, "attrs bold run underline_color null")


def main() -> int:
    return run_demo("r1399_textgrid_underline", body)


if __name__ == "__main__":
    sys.exit(main())
