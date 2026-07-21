#!/usr/bin/env python3
"""R1403 §5.41 — cell-native `TextGrid` OSC-8 hyperlink target.

A terminal cell can now carry an OSC-8 hyperlink — the escape a real
terminal (iTerm2 / kitty / wezterm) turns into underline + hover +
click-to-open. `ls --hyperlink`, compiler diagnostics (`gcc` / `cargo`),
`gh` / `git`, and doc renderers all emit
`ESC ] 8 ; id=<id> ; <uri> ST … ESC ] 8 ; ; ST`; termwiz already parses it,
but the cell model had no place to keep it. `TermCell` now stores a
`HyperlinkId` index into a per-`GridBuffer` interning table (the URI is
stored once, not cloned per cell — R-69.2), resolved through the table at
snapshot / paint time exactly like a palette colour.

The proof is pure DATA over RPC. Each `scene/snapshot` style run now reports
a `hyperlink` object (`{uri, id}`) or `null`:

  * `htg_hyperlink` rows 0-1 are ONE doc-URL link wrapped across the row
    boundary — both segments carry the SAME uri and OSC-8 id "e1", so a
    client recognises two NON-adjacent runs as one logical link (the case a
    pure position-based highlight cannot express, R-69.3.b);
  * row 2 is an anonymous `file://` link (no id), a distinct table entry;
  * every linked cell is drawn blue + single-underlined (the conventional
    affordance, reusing the R1399 underline axis);
  * regression: the R1399 `htg_underline` grid still reports its style axis,
    and its runs carry a `hyperlink` key that is `null` (additive, no
    regression).

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1403_textgrid_hyperlink.py
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

WIN = (680, 900)
LINK_TAG = "htg_hyperlink"
UL_TAG = "htg_underline"

DOC_URI = "https://doc.rust-lang.org/e0499"
FILE_URI = "file:///src/main.rs"


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the hyperlink grid's layout has resolved
        # (cols == 10) AND its 3-row projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, LINK_TAG) or {}).get("cols") == 10
            and len((find_by_tag(s, LINK_TAG) or {}).get("grid_rows", [])) == 3,
            source="paint",
            viewport=WIN,
            desc=f"{LINK_TAG} projection resolved",
        )

        grid = find_by_tag(snap, LINK_TAG)
        assert grid is not None, "hyperlink grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (10, 3), "hyperlink dims 10x3")
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (10, 3), "buffer 10x3")
        rows = grid["grid_rows"]
        assert_eq(len(rows), 3, "hyperlink grid_rows has one entry per row")

        # --- Row 0: the wrapped link's first segment, one contiguous run ---
        r0 = rows[0]
        assert_eq(r0["text"], "rust-lang.", "row0 text is the first URL segment")
        assert_eq(len(r0["runs"]), 1, "row0 is one link run (no boundary)")
        run0 = r0["runs"][0]
        assert_eq((run0["start"], run0["len"]), (0, 10), "row0 run spans all 10 cells")
        # The hyperlink axis is discoverable ($schema / scene-as-data): the key
        # is present and carries the resolved uri + id.
        assert "hyperlink" in run0, "run exposes a hyperlink key (discoverable)"
        assert run0["hyperlink"] is not None, "row0 run is linked"
        assert_eq(run0["hyperlink"]["uri"], DOC_URI, "row0 uri resolves the doc link")
        assert_eq(run0["hyperlink"]["id"], "e1", "row0 link carries OSC-8 id 'e1'")
        # The link affordance: blue foreground + single underline.
        assert_eq(run0["fg"]["kind"], "indexed", "linked cell fg is palette-indexed")
        assert_eq(run0["fg"]["index"], 12, "linked cell fg is bright blue (index 12)")
        assert_eq(run0["attrs"]["underline"], "single", "linked cell is underlined")

        # --- Row 1: the SAME link continues (wrap) + a blank tail ---
        r1 = rows[1]
        # 9 URL chars + the blank tail cell's space (the grid is 10 wide).
        assert_eq(r1["text"], "org/e0499 ", "row1 text is the second URL segment + blank")
        # 9 linked cells + a 1-cell blank tail (the link boundary splits them).
        assert_eq(len(r1["runs"]), 2, "row1 = linked run + blank tail")
        cont = r1["runs"][0]
        assert_eq((cont["start"], cont["len"]), (0, 9), "row1 linked run spans 9 cells")
        assert cont["hyperlink"] is not None, "row1 first run is linked"
        assert_eq(cont["hyperlink"]["uri"], DOC_URI, "row1 uri is the same doc link")
        assert_eq(cont["hyperlink"]["id"], "e1", "row1 shares the OSC-8 id 'e1'")
        # THE grouping proof: two NON-adjacent runs (row 0 and row 1) are one
        # logical link because they share a uri + id — impossible to express
        # with a purely position-based (contiguous) highlight.
        assert_eq(
            (run0["hyperlink"]["uri"], run0["hyperlink"]["id"]),
            (cont["hyperlink"]["uri"], cont["hyperlink"]["id"]),
            "row0 and row1 are the SAME logical link (grouped by id)",
        )
        # The blank tail carries no link.
        tail = r1["runs"][1]
        assert_eq((tail["start"], tail["len"]), (9, 1), "row1 blank tail run")
        assert_eq(tail["hyperlink"], None, "row1 blank tail is un-linked")

        # --- Row 2: an anonymous file:// link (no id), a distinct entry ---
        r2 = rows[2]
        assert_eq(r2["text"], "main.rs:12", "row2 text is the file path")
        assert_eq(len(r2["runs"]), 1, "row2 is one anonymous-link run")
        frun = r2["runs"][0]
        assert frun["hyperlink"] is not None, "row2 is linked"
        assert_eq(frun["hyperlink"]["uri"], FILE_URI, "row2 uri is the file link")
        assert_eq(frun["hyperlink"]["id"], None, "row2 link is anonymous (id null)")
        # It is a DIFFERENT logical link from the doc link (different uri).
        assert frun["hyperlink"]["uri"] != run0["hyperlink"]["uri"], (
            "the file link is distinct from the doc link"
        )
        assert_eq(frun["attrs"]["underline"], "single", "the file link is underlined too")

        # --- Regression: the R1399 underline grid is additive-safe ---
        ul = find_by_tag(snap, UL_TAG)
        assert ul is not None, "htg_underline still present"
        ul_runs = ul["grid_rows"][0]["runs"]
        # The underline style axis is unchanged...
        assert_eq(ul_runs[0]["attrs"]["underline"], "single", "underline grid unregressed")
        # ...and every non-hyperlink run now reports a null hyperlink key.
        assert_eq(ul_runs[0]["hyperlink"], None, "un-linked run reports hyperlink null")
        for i, run in enumerate(ul_runs):
            assert "hyperlink" in run, f"underline run {i} exposes the hyperlink key"
            assert_eq(run["hyperlink"], None, f"underline run {i} hyperlink is null")


def main() -> int:
    return run_demo("r1403_textgrid_hyperlink", body)


if __name__ == "__main__":
    sys.exit(main())
