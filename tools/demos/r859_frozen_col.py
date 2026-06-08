#!/usr/bin/env python3
"""R859 §5.27 — frozen-left-column data-grid E2E.

Drives the `hello-grid-frozen-col` binding via JSON-RPC. First consumer of the
R859 linked-scroll *follower* `ScrollNode` + the frozen-column split in
`pinion_widget_paint::table::view_virtual_table`: a 10,000-row M3 data-grid
whose first two columns (PID, Name) are PINNED against the horizontal scroll
while the metadata columns (CPU%, Memory, …) scroll sideways. Both panes share
the vertical body scroll, so they move in vertical lockstep; only the scrolling
pane's vertical scroll is the primary that publishes bounds, the frozen pane's
is a follower (no measured-viewport oscillation).

The decisive witness is scene-as-data (§2 #7), no pixels required:

  (A) structure — the grid root, the two distinct header bands
      (frozen `gfz_fhrow` + scrolling `gfz_hrow`), the two distinct data-row
      strips (`gfz_frow0` + `gfz_row0`), and both scroll axes are present at
      boot; offsets are 0; the frozen pane sits left of the scrolling pane.
  (B) horizontal scroll => the FREEZE — `scene/scroll` on the horizontal
      scroll advances `offset_x`; the SCROLLED columns' header + body cells
      shift left by EXACTLY `offset_x`, while the FROZEN columns' header +
      body cells DO NOT MOVE (delta 0). Scroll-to-max reveals the right-edge
      column; the frozen column is still at its boot x.
  (C) vertical scroll => LOCKSTEP — `scene/scroll` on the body advances
      `offset_y`; the frozen pane's rows and the scrolling pane's rows slide
      by the SAME amount (a shared row id keeps equal y across panes), while
      NEITHER header moves on either axis. A large vertical scroll drops the
      top row out of BOTH panes together.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the header band is tonally distinct
      from a body row (the grid really painted, header framed).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-frozen-col"
WIN = (560, 440)
N = 10_000
NCOLS = 8
FROZEN_COLS = 2
COL_W = 130
ROW_H = 36
TOTAL_W = NCOLS * COL_W  # 1040 — wider than the window => horizontal scroll
TABLE_TAG = "gfz"
FROZEN_HEADER_TAG = "gfz_fhrow"
SCROLL_HEADER_TAG = "gfz_hrow"
V_SCROLL_TAG = "gfz_scroll"
H_SCROLL_TAG = "gfz_hscroll"
PAUSE = 0.12


def hscroll_node(snap):
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll node present"
    return node


def vscroll_nodes(snap):
    """Every vertical ScrollNode in the snapshot (frozen + scrolling pane)."""
    found = []

    def walk(node):
        if not isinstance(node, dict):
            return
        if node.get("type") == "Scroll" and node.get("axis") == "vertical":
            found.append(node)
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(snap)
    return found


def offset_x(snap) -> int:
    return int(hscroll_node(snap).get("offset_x", -1))


def offset_y(snap) -> int:
    # Both panes share the body state; the first vertical node reports it.
    node = find_by_tag(snap, V_SCROLL_TAG)
    assert node is not None, "vertical body scroll node present"
    return int(node.get("offset_y", -1))


def cell_tag(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def x_of(rects, tag) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][0]


def y_of(rects, tag) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][1]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) structure at boot ───────────────────────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        assert FROZEN_HEADER_TAG in rects, "frozen header band present at boot"
        assert SCROLL_HEADER_TAG in rects, "scrolling header band present at boot"
        assert "gfz_frow0" in rects, "frozen pane row strip present at boot"
        assert "gfz_row0" in rects, "scrolling pane row strip present at boot"

        # One horizontal scroll + two vertical scrolls (one per pane).
        assert_eq(hscroll_node(snap).get("axis"), "horizontal",
                  "scrolling pane's outer scroll reports axis=horizontal")
        assert_eq(len(vscroll_nodes(snap)), 2,
                  "two vertical body scrolls (frozen pane + scrolling pane)")
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset is 0")

        # Frozen columns sit LEFT of the scrolling columns at boot.
        fz_ch0_x = x_of(rects, "gfz_ch0")  # frozen col 0 header
        fz_ch1_x = x_of(rects, "gfz_ch1")  # frozen col 1 header
        sc_ch2_x = x_of(rects, "gfz_ch2")  # first scrolling col header
        assert fz_ch0_x < fz_ch1_x < sc_ch2_x, \
            f"frozen cols left of scrolling cols: {fz_ch0_x} < {fz_ch1_x} < {sc_ch2_x}"
        # Header column-aligned with its body cell at rest (both panes).
        assert_eq(x_of(rects, "gfz_ch0"), x_of(rects, cell_tag(0, 0)),
                  "frozen header col 0 x-aligned with body cell (0,0)")
        assert_eq(x_of(rects, "gfz_ch2"), x_of(rects, cell_tag(0, 2)),
                  "scrolling header col 2 x-aligned with body cell (0,2)")
        # Header bands above the first row (frozen vertically).
        assert y_of(rects, FROZEN_HEADER_TAG) < y_of(rects, "gfz_frow0"), \
            "frozen header above the frozen pane's first row"
        assert y_of(rects, SCROLL_HEADER_TAG) < y_of(rects, "gfz_row0"), \
            "scrolling header above the scrolling pane's first row"

        # Boot reference positions.
        fz_c00_x_boot = x_of(rects, cell_tag(0, 0))
        fz_c01_x_boot = x_of(rects, cell_tag(0, 1))
        sc_c02_x_boot = x_of(rects, cell_tag(0, 2))
        fz_ch0_y_boot = y_of(rects, "gfz_ch0")
        sc_ch2_y_boot = y_of(rects, "gfz_ch2")

        # ── (B) horizontal scroll => the FREEZE ─────────────────────
        D = 150  # < max_x, advances the scrolling pane
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_x(snap), D, "horizontal scroll advanced offset_x")
        assert_eq(offset_y(snap), 0, "vertical offset unchanged by horizontal scroll")
        rects2 = abs_rects_of(snap)

        # Frozen columns DID NOT MOVE.
        assert_eq(x_of(rects2, cell_tag(0, 0)), fz_c00_x_boot,
                  "FROZEN body cell (0,0) x unchanged after horizontal scroll")
        assert_eq(x_of(rects2, cell_tag(0, 1)), fz_c01_x_boot,
                  "FROZEN body cell (0,1) x unchanged after horizontal scroll")
        assert_eq(x_of(rects2, "gfz_ch0"), fz_ch0_x,
                  "FROZEN header col 0 x unchanged after horizontal scroll")
        # Scrolling columns shifted LEFT by exactly offset_x.
        assert_eq(x_of(rects2, cell_tag(0, 2)), sc_c02_x_boot - D,
                  "SCROLLING body cell (0,2) shifted left by exactly offset_x")
        assert_eq(x_of(rects2, "gfz_ch2"), sc_ch2_x - D,
                  "SCROLLING header col 2 shifted left by exactly offset_x")
        # Scrolling header + body still x-aligned (share the same scroll).
        assert_eq(x_of(rects2, "gfz_ch2"), x_of(rects2, cell_tag(0, 2)),
                  "scrolling header col 2 still x-aligned with body cell (0,2)")
        # Headers pinned vertically (Y unchanged) on both panes.
        assert_eq(y_of(rects2, "gfz_ch0"), fz_ch0_y_boot,
                  "frozen header Y unchanged by horizontal scroll")
        assert_eq(y_of(rects2, "gfz_ch2"), sc_ch2_y_boot,
                  "scrolling header Y unchanged by horizontal scroll")

        # Scroll-to-max: reveal the right-edge column; frozen col still put.
        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        rects3 = abs_rects_of(snap)
        max_x = offset_x(snap)
        assert max_x > D, f"scroll-to-max advanced past D ({max_x} > {D})"
        assert "gfz#0_7" in rects3, "right-edge column body cell rendered at max scroll"
        assert_eq(x_of(rects3, "gfz_ch7"), x_of(rects3, cell_tag(0, 7)),
                  "header col 7 x-aligned with body cell (0,7) at max scroll")
        assert_eq(x_of(rects3, cell_tag(0, 0)), fz_c00_x_boot,
                  "FROZEN body cell (0,0) STILL at boot x at max horizontal scroll")
        assert_eq(x_of(rects3, "gfz_ch0"), fz_ch0_x,
                  "FROZEN header col 0 STILL at boot x at max horizontal scroll")

        # Reset horizontal.
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_x(snap), 0, "horizontal scrolled back to 0")
        rects_reset = abs_rects_of(snap)
        assert_eq(x_of(rects_reset, "gfz_ch2"), sc_ch2_x,
                  "scrolling header col 2 back at boot x after reset")

        # ── (C) vertical scroll => LOCKSTEP across panes ────────────
        fz_header_x = x_of(rects_reset, "gfz_ch0")
        fz_header_y = y_of(rects_reset, "gfz_ch0")
        sc_header_y = y_of(rects_reset, "gfz_ch2")
        # A modest scroll keeps a deep row visible in BOTH panes.
        V = ROW_H * 4  # 144 — four rows down
        tf.scroll(V_SCROLL_TAG, to=(0, V))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_y(snap), V, "vertical scroll advanced offset_y")
        assert_eq(offset_x(snap), 0, "horizontal offset unaffected by vertical scroll")
        rects4 = abs_rects_of(snap)
        # A row visible in both panes keeps EQUAL y (vertical lockstep).
        rid = 8  # well within the window after a 4-row scroll
        assert f"gfz_frow{rid}" in rects4, f"frozen pane shows row {rid} after v-scroll"
        assert f"gfz_row{rid}" in rects4, f"scrolling pane shows row {rid} after v-scroll"
        assert_eq(y_of(rects4, f"gfz_frow{rid}"), y_of(rects4, f"gfz_row{rid}"),
                  f"row {rid} at EQUAL y in both panes (vertical lockstep)")
        # Frozen + scrolling cells of the same row share y too.
        assert_eq(y_of(rects4, cell_tag(rid, 0)), y_of(rects4, cell_tag(rid, 2)),
                  f"row {rid} frozen cell + scrolling cell share y (lockstep)")
        # Neither header moved on EITHER axis.
        assert_eq(x_of(rects4, "gfz_ch0"), fz_header_x, "frozen header X unchanged by v-scroll")
        assert_eq(y_of(rects4, "gfz_ch0"), fz_header_y, "frozen header Y unchanged by v-scroll")
        assert_eq(y_of(rects4, "gfz_ch2"), sc_header_y, "scrolling header Y unchanged by v-scroll")

        # Large vertical scroll drops the top row from BOTH panes together.
        tf.scroll(V_SCROLL_TAG, to=(0, 4000))
        time.sleep(PAUSE)
        snap = snap_now()
        rects5 = abs_rects_of(snap)
        assert "gfz_frow0" not in rects5, "top frozen row scrolled out vertically"
        assert "gfz_row0" not in rects5, "top scrolling row scrolled out vertically"
        assert FROZEN_HEADER_TAG in rects5, "frozen header still present after v-scroll (pinned)"
        assert SCROLL_HEADER_TAG in rects5, "scrolling header still present after v-scroll (pinned)"

        # Reset vertical.
        tf.scroll(V_SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_y(snap), 0, "vertical scrolled back to top")

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Header band tonally distinct from a data row (grid really painted).
    # Sample inside the FROZEN pane (near the left edge — always on-screen).
    _, hy, _, hh = rects[FROZEN_HEADER_TAG]
    _, ry, _, rh = rects["gfz_frow0"]
    on_screen_x = 40
    header_pt = (on_screen_x, hy + hh // 2)
    row_pt = (on_screen_x, ry + rh // 2)
    header_px, row_px = sample_png_points(img, [header_pt, row_pt])
    assert header_px != row_px, \
        f"header band tonally distinct from a data row, got {header_px} vs {row_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r859-")) / "frozen.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R859 §5.27 — frozen-left-column data-grid", body))
