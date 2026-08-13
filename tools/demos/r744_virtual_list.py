#!/usr/bin/env python3
"""R744 §5.27 — Model/View virtualization (windowed list) E2E.

Drives the `hello-virtual-list` binding via JSON-RPC. First consumer of
`pinion_widget_paint::virtual_list::view_virtual_list` over the existing
R55.G ScrollNode substrate: a 10,000-row list that materializes scene
nodes for ONLY the visible window (react-window FixedSizeList technique).

The decisive witness is scene-as-data (§2 #7), not pixels: a 10,000-item
list whose `scene/snapshot` reports ~15 row nodes, and whose rendered
band SLIDES to a higher index range as the scroll offset advances — while
the full-height sizer keeps the scroll bound (and the scrollbar thumb
tiny) at the true total extent.

Atomic verification scope (>=30 assertions):

  (A) windowing introspection — at boot only the top ~15 of 10,000 rows
      exist in the tree; the present row-index set matches the windowing
      math exactly; deep rows are absent.
  (B) the window slides — a real `scene/wheel` input then a precise
      `scene/scroll` advance the offset; the present row band shifts to
      the matching higher indices and stays small; scroll-to-bottom
      reaches the last row and the offset clamps to `N*pitch - viewport`
      (proving the sizer drove `max_y`).
  (C) scrollbar peer — the thumb is tiny (sized against the total
      extent), the structural proof the bound is the whole list.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render):
      the windowed rows really paint — adjacent rows show the distinct
      zebra tones (so rows are real, not an empty clip), and the list
      region is tonally distinct from the page surround.
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
    unclipped_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    wait_snap,
)

EXAMPLE = "hello-virtual-list"
WIN = (360, 460)
N = 10_000
PITCH = 32
VP_H = 384  # 12 rows
OVERSCAN = 3
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"
BAR_TAG = "vlist_scrollbar"


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int):
    """Python mirror of `pinion_core::widgets::virtual_list::
    compute_visible_range` — the windowing SSOT. The RPC-observed row
    band is asserted equal to this, tying the live tree to the math."""
    if n == 0 or vp_h == 0 or pitch == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + vp_h
    max_index = n - 1
    first_visible = min(offset // pitch, max_index)
    last_visible = min((bottom - 1) // pitch, max_index)
    first = max(0, first_visible - overscan)
    last = min(last_visible + overscan, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
    """Set of data indices whose `vlist#<i>` row node exists in the tree."""
    out: set[int] = set()
    for tag in unclipped_rects_of(snap):
        if tag.startswith("vlist#"):
            out.add(int(tag.split("#", 1)[1]))
    return out


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot window: ~15 of 10,000 rows, matching the math ──
        rects = unclipped_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        rows = present_rows(snap)
        expected = visible_window(0, VP_H, N, PITCH, OVERSCAN)
        assert_eq(rows, expected, "boot rendered band == windowing math (rows 0..14)")
        assert len(rows) < 30, f"virtualized: only a small window rendered, got {len(rows)} of {N}"
        assert len(rows) >= 12, f"window covers >= the 12-row viewport, got {len(rows)}"
        assert 0 in rows, "row 0 rendered at top"
        assert 14 in rows, "row 14 (last overscan) rendered"
        assert 15 not in rows, "row 15 NOT rendered (below window)"
        assert 5000 not in rows, "a deep row is NOT rendered (the whole point)"
        assert 9999 not in rows, "the last row is NOT rendered at the top"

        # Rows are vertically ordered, one pitch apart (window-absolute y).
        y0 = rects["vlist#0"][1]
        y1 = rects["vlist#1"][1]
        assert_eq(y1 - y0, PITCH, "adjacent rows are exactly one pitch apart")
        for i in sorted(rows)[:-1]:
            assert rects[f"vlist#{i}"][1] < rects[f"vlist#{i + 1}"][1], f"row {i} above {i+1}"

        # ── (C) scrollbar thumb is tiny (bound = total extent) ──────
        bar = find_by_tag(snap, BAR_TAG)
        assert bar is not None, "scrollbar peer present"
        thumb = (bar.get("children") or [None])[0]
        assert isinstance(thumb, dict), "scrollbar has a thumb child"
        thumb_h = int(thumb["rect"]["h"])
        assert thumb_h < VP_H // 8, \
            f"thumb tiny vs viewport (sized against {N}*{PITCH}px total), got {thumb_h}"

        # ── (B) the window slides — real wheel input first ──────────
        tf.wheel(path=SCROLL_TAG, pixels=(0.0, 2000.0))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) > 0,
            viewport=WIN,
            desc="wheel advanced the offset",
        )
        off1 = scroll_offset(snap)
        rows1 = present_rows(snap)
        assert 0 not in rows1, "top row scrolled out of the window after wheel"
        assert_eq(rows1, visible_window(off1, VP_H, N, PITCH, OVERSCAN),
                  "post-wheel band == windowing math at the new offset")
        assert len(rows1) < 30, f"window stays small after wheel, got {len(rows1)}"

        # ── (B) precise programmatic scroll to a deep offset ────────
        tf.scroll(SCROLL_TAG, to=(0, 4000))  # 125 rows down at pitch 32
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == 4000,
            viewport=WIN,
            desc="scroll-to landed at offset 4000",
        )
        rows2 = present_rows(snap)
        expected2 = visible_window(4000, VP_H, N, PITCH, OVERSCAN)
        assert_eq(rows2, expected2, "deep band == windowing math (rows 122..139)")
        assert 130 in rows2, "a mid-band row is rendered"
        assert 0 not in rows2 and 9999 not in rows2, "only the mid band exists"
        assert len(rows2) < 30, f"window still small deep in the list, got {len(rows2)}"

        # ── (B) scroll to the very bottom — last row + clamped max ──
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # past the end → clamps
        max_off = N * PITCH - VP_H  # 320000 - 384 = 319616
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == max_off,
            viewport=WIN,
            desc="offset clamps to N*pitch - viewport (sizer drove max_y)",
        )
        rows3 = present_rows(snap)
        assert 9999 in rows3, "the last row is reachable at the bottom"
        assert_eq(rows3, visible_window(max_off, VP_H, N, PITCH, OVERSCAN),
                  "bottom band == windowing math (rows 9985..9999)")
        assert len(rows3) < 30, f"window small even at the bottom, got {len(rows3)}"

        # ── scroll back to top restores the boot window ─────────────
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == 0,
            viewport=WIN,
            desc="scrolled back to top",
        )
        assert_eq(present_rows(snap), expected, "top window restored exactly")

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Zebra: adjacent rows carry distinct surface tones, so the windowed
    # rows really painted (an empty clip would be one flat colour). Sample
    # each row's right interior (past the left-aligned label, no glyphs).
    def row_bg_point(i: int):
        x, y, w, h = rects[f"vlist#{i}"]
        return (x + w - 8, y + h // 2)

    bg0, bg1, bg2 = sample_png_points(img, [row_bg_point(0), row_bg_point(1), row_bg_point(2)])
    assert bg0 != bg1, f"row 0 / row 1 zebra tones differ (rows painted), got {bg0} vs {bg1}"
    assert bg0 == bg2, f"even rows share a tone (zebra parity), got {bg0} vs {bg2}"

    # The list region is tonally distinct from the page surround: sample a
    # point in the centered window margin (left of the list) vs a row bg.
    lx, ly, lw, lh = rects[LIST_TAG]
    if lx >= 12:
        page_pt = (max(2, lx - 8), ly + lh // 2)
        page = sample_png_points(img, [page_pt])[0]
        assert page != bg0, f"list rows are tonally distinct from the page surround, got {page} vs {bg0}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, unclipped_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r744-")) / "vlist.png"
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
    sys.exit(run_demo("R744 §5.27 — Model/View virtualization (windowed list)", body))
