#!/usr/bin/env python3
"""R775 §5.27 — virtualized data-grid (flex-viewport table) E2E.

Drives the `hello-virtual-table` binding via JSON-RPC. First consumer of
`pinion_widget_paint::table::view_virtual_table`: a 10,000-row M3 data grid
with a FROZEN header above a flex-viewport (AutoSizer, R774) virtualized
body. The body windows against the runtime-measured viewport height; the
header never scrolls.

The decisive witness is scene-as-data (§2 #7): the painted grid reports the
header row plus ~`measured_clip / row_height` data-row strips even though
N = 10,000; an ascending `scene/resize` sweep grows that count; `scene/wheel`
slides the band while the header stays put; deep rows never materialize.

Atomic verification scope (>=30 assertions):

  (A) structure — grid root + frozen header row + per-column headers +
      the body ScrollNode are present at boot; the body is windowed
      (rendered data rows == the windowing math fed the MEASURED clip
      height, far below N); every rendered row carries its NCOLS cells.
  (B) live resize => body row count tracks the measured viewport — an
      ascending `scene/resize` sweep grows the Scroll clip height and the
      rendered data-row count strictly increases (header count stays 1).
  (C) the body slides, header frozen — `scene/wheel` then `scene/scroll`
      advance the offset; the data band shifts to higher indices, the
      header row stays present and unscrolled; scroll-to-bottom reaches
      the last row.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the header band is tonally
      distinct from the body rows (the grid really painted, header framed).
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
    wait_until,
)

EXAMPLE = "hello-virtual-table"
WIN = (400, 480)
N = 10_000
NCOLS = 3
ROW_H = 36
OVERSCAN = 3
TABLE_TAG = "vtbl"
HEADER_TAG = "vtbl_hrow"
SCROLL_TAG = "vtbl_scroll"
PAUSE = 0.12


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int):
    if n == 0 or vp_h <= 0 or pitch == 0:
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
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        # data-row strips are `vtbl_row<id>`; the header is `vtbl_hrow`.
        if tag.startswith("vtbl_row"):
            out.add(int(tag[len("vtbl_row"):]))
    return out


def scroll_node(snap):
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "body scroll node present"
    return node


def scroll_offset(snap) -> int:
    return int(scroll_node(snap).get("offset_y", -1))


def scroll_viewport_h(snap) -> int:
    return int((scroll_node(snap).get("viewport") or {}).get("h", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def resize_to(win_h: int, want_vp: int):
            resp = tf.request("scene/resize", {"width": WIN[0], "height": win_h})
            assert resp is not None and resp.result is not None, "scene/resize accepted"

            def settled():
                s = snap_now()
                return s if scroll_viewport_h(s) == want_vp else None

            return wait_until(settled, desc=f"clip settles at {want_vp} (win {win_h})")

        # ── (A) structure + windowed body at boot ───────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        assert HEADER_TAG in rects, "frozen header row present at boot"
        assert SCROLL_TAG in rects, "body scroll node present at boot"
        for col in range(NCOLS):
            assert f"vtbl_ch{col}" in rects, f"columnheader {col} present"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        boot_vp = scroll_viewport_h(snap)
        assert boot_vp > 0, f"body clip measured > 0, got {boot_vp}"
        rows = present_rows(snap)
        expected = visible_window(0, boot_vp, N, ROW_H, OVERSCAN)
        assert_eq(rows, expected, "boot data band == windowing math at the MEASURED clip")
        assert len(rows) < 30, f"windowed body, not {N} rows: {len(rows)}"
        assert len(rows) >= 8, f"covers the visible band, got {len(rows)}"
        assert 0 in rows, "row 0 rendered at top"
        assert 5000 not in rows, "a deep row is NOT rendered"
        assert 9999 not in rows, "the last row is NOT rendered at the top"

        # Every rendered row carries NCOLS cells `vtbl#<id>_<col>`.
        for rid in sorted(rows)[:5]:
            for col in range(NCOLS):
                assert f"vtbl#{rid}_{col}" in rects, f"row {rid} cell {col} present"

        # Header sits ABOVE the first data row (frozen, not scrolled).
        hy = rects[HEADER_TAG][1]
        r0y = rects["vtbl_row0"][1]
        assert hy < r0y, f"header band is above the first data row ({hy} < {r0y})"

        # ── (B) live resize => body row count tracks measured clip ──
        prev_vp, prev_count = boot_vp, len(rows)
        for win_h in (580, 700, 860):
            # The body grows by exactly the window-height delta (the header
            # band + block padding are fixed), so the measured clip is
            # boot_vp shifted by the delta.
            want_vp = boot_vp + (win_h - WIN[1])
            snap_g = resize_to(win_h, want_vp)
            vp_g = scroll_viewport_h(snap_g)
            assert vp_g > prev_vp, f"clip grew: {vp_g} > {prev_vp} (win {win_h})"
            rows_g = present_rows(snap_g)
            assert_eq(rows_g, visible_window(0, vp_g, N, ROW_H, OVERSCAN),
                      f"resize {win_h}: band == windowing math at MEASURED clip")
            count_g = len(rows_g)
            assert count_g > prev_count, f"taller => more rows: {count_g} > {prev_count}"
            assert count_g < 40, f"still a window: {count_g}"
            assert HEADER_TAG in abs_rects_of(snap_g), "header still present after resize"
            assert 0 in rows_g, "row 0 still at the top of the grown body"
            prev_vp, prev_count = vp_g, count_g

        big_vp = prev_vp

        # ── (C) the body slides, header frozen ──────────────────────
        tf.wheel(path=SCROLL_TAG, pixels=(0.0, 2000.0))
        time.sleep(PAUSE)
        snap = snap_now()
        off1 = scroll_offset(snap)
        assert off1 > 0, f"wheel advanced the body offset, got {off1}"
        rects1 = abs_rects_of(snap)
        assert HEADER_TAG in rects1, "header stays present after wheel (frozen)"
        rows1 = present_rows(snap)
        assert 0 not in rows1, "top data row scrolled out after wheel"
        assert_eq(rows1, visible_window(off1, big_vp, N, ROW_H, OVERSCAN),
                  "post-wheel band == windowing math at the new offset")
        assert len(rows1) < 40, f"body stays windowed after wheel, got {len(rows1)}"

        tf.scroll(SCROLL_TAG, to=(0, 4000))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(scroll_offset(snap), 4000, "scroll-to landed at offset 4000")
        rows2 = present_rows(snap)
        assert_eq(rows2, visible_window(4000, big_vp, N, ROW_H, OVERSCAN),
                  "deep band == windowing math")
        assert 0 not in rows2 and 9999 not in rows2, "only the mid band exists"

        tf.scroll(SCROLL_TAG, to=(0, 10**9))
        time.sleep(PAUSE)
        snap = snap_now()
        max_off = N * ROW_H - big_vp
        assert_eq(scroll_offset(snap), max_off,
                  "offset clamps to N*row_h - measured_clip (sizer drove max_y)")
        rows3 = present_rows(snap)
        assert 9999 in rows3, "the last row is reachable at the bottom"
        assert HEADER_TAG in abs_rects_of(snap), "header still frozen at the bottom"

        tf.scroll(SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(scroll_offset(snap), 0, "scrolled back to top")

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Header band tonally distinct from a data row (grid really painted).
    hx, hy, hw, hh = rects[HEADER_TAG]
    rx, ry, rw, rh = rects["vtbl_row0"]
    header_pt = (hx + hw // 2, hy + hh // 2)
    row_pt = (rx + rw - 8, ry + rh // 2)
    header_px, row_px = sample_png_points(img, [header_pt, row_pt])
    assert header_px != row_px, \
        f"header band tonally distinct from a data row, got {header_px} vs {row_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r775-")) / "vtable.png"
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
    sys.exit(run_demo("R775 §5.27 — virtualized data-grid (flex-viewport table)", body))
