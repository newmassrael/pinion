#!/usr/bin/env python3
"""R896 §5.27 — horizontal scroll on the EDITABLE data grid.

Drives `hello-data-grid` via JSON-RPC. R896 converges the editable grid onto
the read-only grids' R784 horizontal-scroll wrap
(`pinion_widget_paint::table::h_scrolled_column`) instead of hand-rolling a
second copy: the six columns are now `690 px` wide — wider than the
`GRID_VIEWPORT_W` (370 px) band — so the header + rows share ONE horizontal
scroll and slide sideways together, the trailing Scale / Active columns
revealing on scroll while the editable cursor / edit latch (SOURCE-keyed) stay
put. The witness is scene-as-data (§2 #7), no pixels required:

  (A) structure — the grid root, the `dg_header` band, and the horizontal
      scroll node (`axis = "horizontal"`, `offset_x = 0` at boot) are present;
      the columns overflow (the leading Asset column is on-screen, the
      trailing Active column is laid out beyond the viewport).              [10]
  (B) horizontal scroll => header/body sync — `scene/scroll` advances
      `offset_x`; a fully-visible column's HEADER cell and a BODY cell shift
      left by EXACTLY `offset_x` and stay x-aligned (one shared scroll), while
      their Y is unchanged (the header is pinned vertically).                 [8]
  (C) reveal-on-scroll + clamp — scroll-to-max clamps to `total_w -
      viewport_w`, reveals the trailing Active column, and clips out the
      leading Asset column.                                                   [6]
  (D) scroll is orthogonal to edit state — the focused cell / model are
      SOURCE-keyed, unchanged by a horizontal scroll; AI reads `offset_x`
      straight off the scroll node and scrolls back to rest.                  [7]
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (460, 348)
GRID = "data_grid"
HEADER = "dg_header"
H_SCROLL = "data_grid_hscroll"
# The h-scroll content = the R937 grip-handle column (HANDLE_W = 22) + the six
# data columns (COL_W sum = 690) = 712, all wider than the 370 px viewport. (The
# handle scrolls with the columns, so it is part of the scrollable extent — the
# clamp is `712 - viewport_w`, not `690 - viewport_w`.)
TOTAL_W = 22 + 160 + 110 + 100 + 100 + 100 + 120  # HANDLE_W + COL_W sum = 712


def hscroll_node(snap):
    node = find_by_tag(snap, H_SCROLL)
    assert node is not None, "horizontal scroll node present"
    return node


def offset_x(snap) -> int:
    return int(hscroll_node(snap).get("offset_x", -1))


def vscroll_offset_y(snap) -> int:
    node = find_by_tag(snap, "data_grid_vscroll")
    return int(node.get("offset_y", -1)) if node is not None else -1


def hviewport_w(snap) -> int:
    return int((hscroll_node(snap).get("viewport") or {}).get("w", -1))


def hviewport_x(snap) -> int:
    return int((hscroll_node(snap).get("viewport") or {}).get("x", -1))


def x_visible(rect, vp_x, vp_w) -> bool:
    """`abs_rects_of` reports window-absolute geometry WITHOUT clipping, so a
    column is on-screen iff its x-span overlaps the horizontal viewport."""
    x, _, w, _ = rect
    return x < vp_x + vp_w and x + w > vp_x


def col_x(rects, tag) -> int:
    assert tag in rects, f"{tag} present in the laid-out scene"
    return rects[tag][0]


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) structure + overflow at boot ────────────────────────
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)
        assert GRID in rects, "grid root present at boot"
        assert HEADER in rects, "dg_header band present at boot"
        assert_eq(hscroll_node(snap).get("axis"), "horizontal",
                  "scroll node reports axis=horizontal")
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")

        vp_w = hviewport_w(snap)
        vp_x = hviewport_x(snap)
        assert vp_w > 0, f"horizontal viewport measured > 0, got {vp_w}"
        assert vp_w < TOTAL_W, f"viewport ({vp_w}) narrower than columns ({TOTAL_W})"
        assert TOTAL_W - vp_w > 0, "there is a non-zero horizontal extent to scroll"
        # Leading Asset column header on-screen; trailing Active beyond it.
        assert x_visible(rects["data_grid#h0"], vp_x, vp_w), "Asset column on-screen at boot"
        assert not x_visible(rects["data_grid#h4"], vp_x, vp_w), \
            "Active column laid out beyond the viewport at boot (overflow)"
        # Header band sits above the first data row.
        assert rects[HEADER][1] < rects["data_grid#0_0"][1], "header band above first row"

        # ── (B) horizontal scroll => header/body sync ───────────────
        # Type (col 1) is fully visible at both offset 0 and D=80.
        h_h1_boot = col_x(rects, "data_grid#h1")
        c_01_boot = col_x(rects, "data_grid#0_1")
        assert_eq(h_h1_boot, c_01_boot, "Type header x-aligned with body cell (0,1) at rest")
        h1_y_boot = rects["data_grid#h1"][1]
        c01_y_boot = rects["data_grid#0_1"][1]

        D = 80
        tf.scroll(H_SCROLL, to=(D, 0))
        snap = wait_snap(tf, lambda s: offset_x(s) == D, viewport=WIN,
                         desc="horizontal scroll advanced offset_x to 80")
        assert_eq(offset_x(snap), D, "offset_x reads back exactly the requested 80")
        rects2 = abs_rects_of(snap)
        h_h1 = col_x(rects2, "data_grid#h1")
        c_01 = col_x(rects2, "data_grid#0_1")
        assert_eq(h_h1, c_01, "Type header still x-aligned with body cell (0,1) after scroll")
        assert_eq(h_h1, h_h1_boot - D, "Type header shifted left by exactly offset_x")
        assert_eq(c_01, c_01_boot - D, "body cell (0,1) shifted left by exactly offset_x")
        assert_eq(rects2["data_grid#h1"][1], h1_y_boot, "header Y unchanged by horizontal scroll")
        assert_eq(rects2["data_grid#0_1"][1], c01_y_boot, "body row Y unchanged by horizontal scroll")

        # ── (C) reveal-on-scroll + clamp to max ─────────────────────
        tf.scroll(H_SCROLL, to=(10 ** 9, 0))
        max_x = TOTAL_W - vp_w
        snap = wait_snap(tf, lambda s: offset_x(s) == TOTAL_W - hviewport_w(s), viewport=WIN,
                         desc="offset_x clamps to total_w - viewport_w")
        assert_eq(offset_x(snap), max_x, "scroll-to-max clamps to total_w - viewport_w")
        rects3 = abs_rects_of(snap)
        assert x_visible(rects3["data_grid#h4"], hviewport_x(snap), hviewport_w(snap)), \
            "Active column revealed at max scroll"
        assert not x_visible(rects3["data_grid#h0"], hviewport_x(snap), hviewport_w(snap)), \
            "Asset column clipped out at max scroll"
        # The Active body cell rect now exists on-screen (a click could reach it).
        assert "data_grid#0_4" in rects3, "Active body cell laid out at max scroll"
        assert x_visible(rects3["data_grid#0_4"], hviewport_x(snap), hviewport_w(snap)), \
            "Active body cell (0,4) on-screen at max scroll (clickable)"

        # ── (D) scroll is orthogonal to edit state ──────────────────
        # Park the cursor on a cell, then scroll: the SOURCE-keyed focus and
        # the model are untouched (scroll moves pixels, not state).
        tf.intervene("/external/focused_row", 2)
        tf.intervene("/external/focused_col", 1)
        assert_eq(tf.query("/external/focused_row"), 2, "cursor parked at row 2")
        assert_eq(tf.query("/external/focused_col"), 1, "cursor parked at col 1")
        seed_01 = tf.query("/external/value.0.1")
        tf.scroll(H_SCROLL, to=(0, 0))
        wait_snap(tf, lambda s: offset_x(s) == 0, viewport=WIN, desc="scrolled back to rest")
        assert_eq(tf.query("/external/focused_row"), 2, "focus row survives the scroll")
        assert_eq(tf.query("/external/focused_col"), 1, "focus col survives the scroll")
        assert_eq(tf.query("/external/value.0.1"), seed_01, "model value survives the scroll")
        assert_eq(offset_x(tf.snapshot(source="paint", viewport=WIN)), 0,
                  "AI reads offset_x straight off the scroll node")

        # ── (E) R896.1 — vertical scroll catches grouped overflow ───
        # Grouping by Asset (4 distinct: Hero/Tree/Coin/Boss) interleaves 4
        # group headers + 4 data rows = 8 rows, taller than the band. Before
        # R896.1 the bottom rows clipped silently (no v-scroll); now the inner
        # vertical scroll catches the overflow so every grouped row is reachable.
        groups = tf.invoke("/external/set_group", "0")
        assert_eq(groups, 4, "Asset groups into 4 single-member runs (8 rows > band)")
        snap = wait_snap(tf, lambda s: find_by_tag(s, "data_grid_vscroll") is not None,
                         viewport=WIN, desc="vertical scroll node present when grouped")
        assert_eq(vscroll_offset_y(snap), 0, "grouped body starts at the top")
        tf.scroll("data_grid_vscroll", to=(0, 10 ** 9))  # clamp to bottom
        snap = wait_snap(tf, lambda s: vscroll_offset_y(s) > 0, viewport=WIN,
                         desc="grouped rows overflow => vertical scroll engages (not clipped)")
        assert vscroll_offset_y(snap) > 0, "v-scroll advanced: bottom grouped rows are reachable"
        tf.invoke("/external/set_group", None)  # back to flat
        wait_snap(tf, lambda s: vscroll_offset_y(s) <= 0, viewport=WIN,
                  desc="ungrouped body fits the band again (no overflow)")


if __name__ == "__main__":
    sys.exit(run_demo("R896 §5.27 — horizontal scroll on the editable data grid", body))
