#!/usr/bin/env python3
"""R784 §5.45 — horizontal scroll + frozen header at scale E2E.

Drives the `hello-grid-hscroll` binding via JSON-RPC. First consumer of the
R784 `ScrollAxis::Horizontal` layout substrate + the nested
outer-horizontal / inner-vertical composition in
`pinion_widget_paint::table::view_virtual_table`: a 10,000-row M3 data-grid
whose 8 columns (NCOLS x COL_W = 1040) are wider than the window, so the grid
scrolls horizontally. The header band and the body share ONE outer horizontal
scroll, so the header tracks the body's horizontal offset exactly while
staying vertically pinned above the inner vertical body scroll.

The decisive witness is scene-as-data (§2 #7), no pixels required:

  (A) structure + axes — the grid root, the frozen header row, the inner
      vertical body scroll (`axis = "vertical"`), and the outer horizontal
      scroll (`axis = "horizontal"`) are all present at boot; columns
      overflow (a right-edge column is clipped out of the viewport); the
      header is column-aligned with the body cells.
  (B) horizontal scroll => frozen-header sync — `scene/scroll` on the outer
      horizontal scroll advances `offset_x`; a fully-visible column's HEADER
      cell and a BODY cell shift left by EXACTLY `offset_x` and stay
      x-aligned with each other (they share one scroll), while the header's Y
      is unchanged (pinned vertically); scroll-to-max clamps to
      `total_w - viewport_w`, reveals the right-edge column, and clips out
      the left-edge column.
  (C) vertical scroll independence — `scene/scroll` on the body advances
      `offset_y`; the rows slide and the header stays put on BOTH axes (same
      X, same Y) — the two scroll axes are orthogonal.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the header band is tonally distinct
      from a body row (the grid really painted, header framed).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
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
    wait_snap,
)

EXAMPLE = "hello-grid-hscroll"
WIN = (520, 440)
N = 10_000
NCOLS = 8
COL_W = 130
ROW_H = 36
TOTAL_W = NCOLS * COL_W  # 1040 — wider than the window => horizontal scroll
TABLE_TAG = "ghs"
HEADER_TAG = "ghs_hrow"
V_SCROLL_TAG = "ghs_scroll"
H_SCROLL_TAG = "ghs_hscroll"


def hscroll_node(snap):
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "outer horizontal scroll node present"
    return node


def vscroll_node(snap):
    node = find_by_tag(snap, V_SCROLL_TAG)
    assert node is not None, "inner vertical body scroll node present"
    return node


def offset_x(snap) -> int:
    return int(hscroll_node(snap).get("offset_x", -1))


def offset_y(snap) -> int:
    return int(vscroll_node(snap).get("offset_y", -1))


def hviewport_w(snap) -> int:
    return int((hscroll_node(snap).get("viewport") or {}).get("w", -1))


def hviewport_x(snap) -> int:
    return int((hscroll_node(snap).get("viewport") or {}).get("x", -1))


def x_visible(rect, vp_x, vp_w) -> bool:
    """`abs_rects_of` reports window-absolute geometry WITHOUT clipping, so
    a column is on-screen iff its x-span overlaps the horizontal viewport."""
    x, _, w, _ = rect
    return x < vp_x + vp_w and x + w > vp_x


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) structure + axes at boot ────────────────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        assert HEADER_TAG in rects, "frozen header row present at boot"

        # The two single-axis scrolls, each reporting its axis (R784).
        assert_eq(hscroll_node(snap).get("axis"), "horizontal",
                  "outer scroll reports axis=horizontal")
        assert_eq(vscroll_node(snap).get("axis"), "vertical",
                  "inner body scroll reports axis=vertical")
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset is 0")

        # Columns overflow the window: the left column is on-screen, the
        # right-edge column is laid out beyond the horizontal viewport.
        vp_w = hviewport_w(snap)
        vp_x = hviewport_x(snap)
        assert vp_w > 0, f"horizontal viewport measured > 0, got {vp_w}"
        assert vp_w < TOTAL_W, f"viewport ({vp_w}) narrower than columns ({TOTAL_W})"
        assert x_visible(rects["ghs_ch0"], vp_x, vp_w), "leftmost column on-screen at boot"
        # R1523 — the column axis is WINDOWED, so the right-edge column is not
        # in the tree at all at boot. Before R1523 it was built and merely
        # positioned beyond the viewport, and this demo asserted exactly that
        # (`not x_visible(ghs_ch7)`) — the assertion was the record of the
        # un-windowed axis. The overflow premise it was there to witness is
        # carried by `vp_w < TOTAL_W` above plus the `max_x` clamp below.
        assert "ghs_ch7" not in rects, \
            "right-edge column windowed OUT at boot (R1523) — not merely off-screen"
        # Row 0 rendered, header above it (frozen vertically).
        assert "ghs_row0" in rects, "first data row rendered at boot"
        assert rects[HEADER_TAG][1] < rects["ghs_row0"][1], "header band above the first row"

        # Header column-aligned with the body cells at rest (col 2, row 0;
        # col 2 is fully visible at both offset 0 and the 200 we use below).
        def col_x(rs, tag):
            assert tag in rs, f"{tag} present"
            return rs[tag][0]

        h_ch2_boot = col_x(rects, "ghs_ch2")
        c_02_boot = col_x(rects, "ghs#0_2")
        assert_eq(h_ch2_boot, c_02_boot, "header col 2 x-aligned with body cell (0,2) at rest")
        h_ch2_y_boot = rects["ghs_ch2"][1]
        c_02_y_boot = rects["ghs#0_2"][1]

        # ── (B) horizontal scroll => frozen-header sync ─────────────
        D = 200  # < max_x (1040 - vp_w), keeps col 2 fully visible
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == D, viewport=WIN,
            desc="horizontal scroll advanced offset_x",
        )
        assert_eq(offset_y(snap), 0, "vertical offset unchanged by horizontal scroll")
        rects2 = abs_rects_of(snap)

        h_ch2 = col_x(rects2, "ghs_ch2")
        c_02 = col_x(rects2, "ghs#0_2")
        # Header and body cell STILL x-aligned — they share one scroll.
        assert_eq(h_ch2, c_02, "header col 2 still x-aligned with body cell (0,2) after h-scroll")
        # Both shifted LEFT by exactly offset_x (frozen-header sync).
        assert_eq(h_ch2, h_ch2_boot - D, "header col 2 shifted left by exactly offset_x")
        assert_eq(c_02, c_02_boot - D, "body cell (0,2) shifted left by exactly offset_x")
        # Header pinned vertically (Y unchanged); body row Y unchanged too
        # (offset_y still 0).
        assert_eq(rects2["ghs_ch2"][1], h_ch2_y_boot, "header Y unchanged by horizontal scroll")
        assert_eq(rects2["ghs#0_2"][1], c_02_y_boot, "body row Y unchanged by horizontal scroll")
        # The header band itself is still present after the h-scroll.
        assert HEADER_TAG in rects2, "header row still present after h-scroll"

        # Scroll-to-max: clamps to total_w - viewport_w, reveals the
        # right-edge column, clips out the left-edge column.
        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == TOTAL_W - hviewport_w(s), viewport=WIN,
            desc="offset_x clamps to total_w - viewport_w",
        )
        rects3 = abs_rects_of(snap)
        assert x_visible(rects3["ghs_ch7"], vp_x, vp_w), \
            "right-edge column on-screen at max horizontal scroll"
        # R1523 — and the left-edge column has left the tree, not just the
        # viewport: scrolling the column window is what makes the far column
        # above appear at all.
        assert "ghs_ch0" not in rects3, \
            "left-edge column windowed OUT at max horizontal scroll"
        # The right-edge body cell is also revealed and column-aligned.
        assert_eq(rects3["ghs_ch7"][0], rects3["ghs#0_7"][0],
                  "header col 7 x-aligned with body cell (0,7) at max scroll")

        # Reset horizontal.
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == 0, viewport=WIN,
            desc="horizontal scrolled back to 0",
        )
        rects_reset = abs_rects_of(snap)
        assert_eq(rects_reset["ghs_ch2"][0], h_ch2_boot, "header col 2 back at boot x after reset")

        # ── (C) vertical scroll independence ────────────────────────
        header_x_before = rects_reset["ghs_ch2"][0]
        header_y_before = rects_reset["ghs_ch2"][1]
        tf.scroll(V_SCROLL_TAG, to=(0, 2000))
        snap = wait_snap(
            tf, lambda s: offset_y(s) == 2000, viewport=WIN,
            desc="vertical scroll advanced offset_y",
        )
        assert_eq(offset_x(snap), 0, "horizontal offset unaffected by vertical scroll")
        rects4 = abs_rects_of(snap)
        assert "ghs_row0" not in rects4, "top data row scrolled out vertically"
        assert HEADER_TAG in rects4, "header still present after vertical scroll (frozen)"
        # Header unchanged on BOTH axes — the two scrolls are orthogonal.
        assert_eq(rects4["ghs_ch2"][0], header_x_before, "header X unchanged by vertical scroll")
        assert_eq(rects4["ghs_ch2"][1], header_y_before, "header Y unchanged by vertical scroll")

        tf.scroll(V_SCROLL_TAG, to=(0, 0))
        wait_snap(
            tf, lambda s: offset_y(s) == 0, viewport=WIN,
            desc="vertical scrolled back to top",
        )

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Header band tonally distinct from a data row (grid really painted).
    # The header / row strips are TOTAL_W wide (they overflow the window),
    # so sample a fixed on-screen x near the left edge rather than the
    # off-screen geometric centre.
    _, hy, _, hh = rects[HEADER_TAG]
    _, ry, _, rh = rects["ghs_row0"]
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
    out = Path(tempfile.mkdtemp(prefix="pinion-r784-")) / "hscroll.png"
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
    sys.exit(run_demo("R784 §5.45 — horizontal scroll + frozen header", body))
