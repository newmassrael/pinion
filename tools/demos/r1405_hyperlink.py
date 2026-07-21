#!/usr/bin/env python3
"""R1405 §5.41 §5.35 — OSC-8 hyperlink hover / click interaction over TextGrid.

The R-69.3 layer on the R1403 hyperlink data model (sprag PINION-PR71). A few
lines of terminal output carry OSC-8 links; a REAL hover (`scene/hover`, driving
the new `wants_hover_move` seam end-to-end through the router) over a link cell:

  * lights the link's whole id-group — every cell sharing the hovered cell's
    HyperlinkId, so a link split across a soft wrap lights as ONE logical
    target (R-71.2). The highlight is cell reverse-video, so `scene/snapshot`
    grid_rows reports it — the SAME id-group lit across the wrap, verified with
    no pixel;
  * the oracle reports the hovered link's uri / id / id-group size (what a
    snapshot cannot);
  * clicking (`scene/click`, the R1401 press channel) activates the URI
    (R-71.3) — the consumer opens it; pinion owns the affordance + the seam.

The pointer/hand cursor (R-71.1, CursorHint::Pointer) has no snapshot field
(`cursor_hint` is a Rust witness) — the demo unit test
`view_sets_the_pointer_cursor_only_while_a_link_is_hovered` covers it.

Run from the workspace root:
    cargo build -p hello-hyperlink --release
    python3 tools/demos/r1405_hyperlink.py
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

WIN = (560, 200)
GRID = "links"
ORACLE = "/external"
GRID_POS = (16, 44)
CELL_W, CELL_H = 8, 16

DOC_URI = "https://doc.rust-lang.org/book"
FILE_URI = "file:///home/user/src/main.rs"
GH_URI = "https://github.com/org/repo/issues/42"


def cell_xy(col: int, row: int) -> tuple[int, int]:
    """The window pixel at cell (col, row)'s centre."""
    return (GRID_POS[0] + col * CELL_W + CELL_W // 2, GRID_POS[1] + row * CELL_H + CELL_H // 2)


def run_at(row: dict, col: int) -> dict:
    for run in row["runs"]:
        if run["start"] <= col < run["start"] + run["len"]:
            return run
    raise AssertionError(f"column {col} is covered by some run")


def reverse_at(snap, col: int, row: int) -> bool:
    return run_at(find_by_tag(snap, GRID)["grid_rows"][row], col)["attrs"]["reverse"]


def hover_uri(tf) -> object:
    return tf.query(f"{ORACLE}/hover_uri")


def body() -> None:
    with RpcSubprocess("hello-hyperlink") as tf:
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == 40,
            source="paint",
            viewport=WIN,
            desc="link grid resolved",
        )

        # --- boot: nothing hovered / activated ---
        assert_eq(find_by_tag(snap, GRID)["cols"], 40, "grid is 40 cols")
        assert_eq(tf.query(f"{ORACLE}/link_count"), 3, "three interned links")
        assert_eq(tf.query(f"{ORACLE}/hover_index"), None, "boot: nothing hovered")
        assert_eq(tf.query(f"{ORACLE}/activated_uri"), None, "boot: nothing activated")

        # --- REAL hover over the doc link (row 0, col 6): drives wants_hover_move
        #     end-to-end through the router. ---
        tf.hover(at=cell_xy(6, 0))
        hot = wait_snap(
            tf,
            lambda s: reverse_at(s, 6, 0),
            source="paint",
            viewport=WIN,
            desc="hovering the doc link lights it",
        )
        assert_eq(tf.query(f"{ORACLE}/hover_index"), 0, "doc link hovered (index 0)")
        assert_eq(hover_uri(tf), DOC_URI, "hovered uri is the doc link")
        assert_eq(tf.query(f"{ORACLE}/hover_id"), "doc", "hovered link's OSC-8 id")
        assert_eq(tf.query(f"{ORACLE}/hover_group_size"), len("rust-lang.org"), "doc id-group size")
        # Its cells reverse-video; a plain (non-link) cell does not.
        assert reverse_at(hot, 6, 0), "doc link cell 6 lit"
        assert reverse_at(hot, 18, 0), "doc link cell 18 lit"
        assert not reverse_at(hot, 0, 0), "the plain 'docs' label is not lit"

        # --- THE wrap proof: hover the GitHub link on ROW 1; its id-group
        #     lights on BOTH row 1 and row 2 (one logical link across the wrap). ---
        tf.hover(at=cell_xy(6, 1))
        wrap = wait_snap(
            tf,
            lambda s: reverse_at(s, 6, 1) and reverse_at(s, 6, 2),
            source="paint",
            viewport=WIN,
            desc="hovering the wrapped link lights both rows",
        )
        assert_eq(tf.query(f"{ORACLE}/hover_index"), 2, "github link hovered (index 2)")
        assert_eq(tf.query(f"{ORACLE}/hover_id"), "gh", "github link's OSC-8 id")
        assert reverse_at(wrap, 6, 1), "row1 segment of the wrapped link lit"
        assert reverse_at(wrap, 6, 2), "row2 segment of the SAME link lit (the wrap group)"
        # The doc link on row 0 is NOT lit now (a different id-group).
        assert not reverse_at(wrap, 6, 0), "the doc link is not part of this id-group"
        assert_eq(
            tf.query(f"{ORACLE}/hover_group_size"),
            len("github.com/org/repo/") + len("issues/42"),
            "the wrap group counts both segments",
        )

        # --- hover the ANONYMOUS file link (row 3, col 6): id is null ---
        tf.hover(at=cell_xy(6, 3))
        anon = wait_snap(
            tf,
            lambda s: reverse_at(s, 6, 3),
            source="paint",
            viewport=WIN,
            desc="hovering the file link lights it",
        )
        assert_eq(tf.query(f"{ORACLE}/hover_index"), 1, "file link hovered (index 1)")
        assert_eq(hover_uri(tf), FILE_URI, "hovered uri is the file link")
        assert_eq(tf.query(f"{ORACLE}/hover_id"), None, "the file link is anonymous (id null)")
        assert reverse_at(anon, 6, 3), "the file link cell is lit"
        assert not reverse_at(anon, 6, 0), "the doc link is not lit while the file link is"

        # --- hover a PLAIN cell -> nothing hovered (hover forwards, resolves None) ---
        tf.hover(at=cell_xy(0, 0))
        plain = wait_snap(
            tf,
            lambda s: not reverse_at(s, 6, 1),
            source="paint",
            viewport=WIN,
            desc="hovering off a link clears the highlight",
        )
        assert_eq(tf.query(f"{ORACLE}/hover_index"), None, "no link hovered over a plain cell")
        assert not reverse_at(plain, 6, 1), "the wrapped link is no longer lit"
        assert not reverse_at(plain, 6, 3), "the file link is no longer lit either"

        # --- CLICK the file link (row 3, col 6) -> activation ---
        tf.click(at=cell_xy(6, 3))
        wait_snap(
            tf,
            lambda _s: tf.query(f"{ORACLE}/activated_uri") == FILE_URI,
            source="paint",
            viewport=WIN,
            desc="clicking the file link activates it",
        )
        assert_eq(tf.query(f"{ORACLE}/activated_uri"), FILE_URI, "clicked link activated")
        assert_eq(tf.query(f"{ORACLE}/activated_index"), 1, "activated the file link (index 1)")

        # --- AI-first, no-pixel: intervene hover_index + invoke activate ---
        tf.intervene(f"{ORACLE}/hover_index", 2)
        assert_eq(tf.query(f"{ORACLE}/hover_index"), 2, "intervene set the hover")
        assert_eq(hover_uri(tf), GH_URI, "intervened hover resolves the uri")
        tf.intervene(f"{ORACLE}/hover_index", None)
        assert_eq(tf.query(f"{ORACLE}/hover_index"), None, "intervene cleared the hover")
        assert_eq(tf.invoke(f"{ORACLE}/activate", 0), DOC_URI, "invoke activate returns the uri")
        assert_eq(tf.query(f"{ORACLE}/activated_uri"), DOC_URI, "activate recorded it")


if __name__ == "__main__":
    sys.exit(run_demo("r1405_hyperlink", body))
