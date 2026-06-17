#!/usr/bin/env python3
"""R977 §5.41 — cell-native `TextGrid` alternate screen (S5 slice 5).

The fifth S5 data-model slice: the screen-kind discriminator. A terminal
has two screen buffers — the **main** scrollback shell surface and the
**alternate** cleared, no-scrollback surface a fullscreen app (vim, htop,
a pager) switches to (DECSET 1049). A projection holds exactly the screen
the producer is currently displaying, tagged with which one it is
(`ScreenKind` = Main / Alternate). pinion does NOT hold the inactive
buffer — the producer owns both and projects the active one (R969).

The `hello-textgrid` window grows a seventh grid, `htg_alt`, tagged as the
alternate screen: a tiny fullscreen-app projection (a file line + a
reverse-video status bar) with its own block cursor. Every other grid
stays on the main screen.

The proof is pure DATA over RPC (glyph paint is still a follow-up round).
Each `scene/snapshot` of a TextGrid now carries `screen` (`main` /
`alternate`), so a client reads the terminal's mode without inferring it
from content:

  * `htg_alt` reports `screen == "alternate"` and its own visible BLOCK
    cursor — distinct from `htg_cursor`'s BAR on the main screen, proving
    each screen carries its own cursor (R975);
  * its status bar row is reverse-video (reusing R974 SGR attrs);
  * every other grid reports `screen == "main"` (the additive default), so
    R972-R976 demos are regression-free.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r977_textgrid_altscreen.py
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
ALT_TAG = "htg_alt"

# Every grid except htg_alt is on the main screen.
MAIN_GRIDS = ("htg_default", "htg_measured", "htg_content", "htg_attrs", "htg_cursor", "htg_wide")


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the alt grid's layout has resolved
        # (cols == 8) AND its 2-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, ALT_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, ALT_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{ALT_TAG} projection resolved",
        )

        # --- htg_alt: the alternate screen ---
        alt = find_by_tag(snap, ALT_TAG)
        assert alt is not None, "alt grid present in paint scene"
        assert_eq((alt["cols"], alt["rows"]), (8, 2), "alt dims 8x2")
        assert_eq((alt["buffer_cols"], alt["buffer_rows"]), (8, 2), "alt buffer 8x2")
        # The headline: this projection is the alternate screen.
        assert_eq(alt["screen"], "alternate", "htg_alt is the alternate screen")

        # It carries its own cursor — a visible block at home.
        cur = alt["cursor"]
        assert_eq((cur["col"], cur["row"]), (0, 0), "alt cursor at home")
        assert_eq(cur["shape"], "block", "alt cursor is a block")
        assert_eq(cur["visible"], True, "alt cursor is visible")

        rows = alt["grid_rows"]
        assert_eq(rows[0]["text"], "file.txt", "alt row0 = file line")
        assert_eq(rows[1]["text"], " 1,1 Top", "alt row1 = status bar")
        # The status bar is reverse-video (one coalesced reverse run).
        status = rows[1]["runs"]
        assert_eq(len(status), 1, "alt status bar is one reverse run")
        assert_eq(status[0]["attrs"]["reverse"], True, "alt status bar is reversed")
        assert_eq((status[0]["start"], status[0]["len"]), (0, 8), "alt status spans the row")
        # The content row is NOT reversed.
        assert_eq(rows[0]["runs"][0]["attrs"]["reverse"], False, "alt content not reversed")

        # --- Contrast: htg_cursor is the MAIN screen with a BAR cursor ---
        main_cursor_grid = find_by_tag(snap, "htg_cursor")
        assert_eq(main_cursor_grid["screen"], "main", "htg_cursor is the main screen")
        assert_eq(main_cursor_grid["cursor"]["shape"], "bar", "main screen cursor is a bar")
        # Two screens, two independent cursors (block on alt, bar on main).
        assert main_cursor_grid["cursor"]["shape"] != cur["shape"], "each screen has its own cursor"

        # --- Regression: every other grid is the main screen ---
        for tag in MAIN_GRIDS:
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            assert_eq(other["screen"], "main", f"{tag} is the main screen")

        # --- Regression: R974 attrs / R976 width fields untouched ---
        content = find_by_tag(snap, "htg_content")
        assert_eq(content["grid_rows"][0]["runs"][0]["width"], "narrow", "htg_content still narrow")
        wide = find_by_tag(snap, "htg_wide")
        wide_widths = [r["width"] for r in wide["grid_rows"][0]["runs"][:2]]
        assert_eq(wide_widths, ["wide", "trailer"], "htg_wide still wide/trailer")


def main() -> int:
    return run_demo("r977_textgrid_altscreen", body)


if __name__ == "__main__":
    sys.exit(main())
