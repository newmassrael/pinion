#!/usr/bin/env python3
"""R745 §5.27 — Model/View variable-height virtualization E2E.

Drives the `hello-variable-list` binding via JSON-RPC. Consumer of
`pinion_widget_paint::virtual_list::view_variable_virtual_list` — the
variable-pitch peer of R744's fixed `view_virtual_list`. A 10,000-row list
whose rows have FOUR DIFFERENT heights (cycling 28/44/60/76 px) is
windowed via a prefix-sum offset table searched in O(log n) (react-window
VariableSizeList technique), over the existing R55.G ScrollNode substrate.

The decisive witness is scene-as-data (§2 #7), not pixels:

  (A) windowing introspection — at boot only a small window of the 10,000
      rows exists; the present row-index set matches the variable
      windowing math exactly; deep rows are absent.
  (B) rows are sized + positioned by the prefix-sum table — each rendered
      row's height equals its modeled height, and adjacent row tops differ
      by exactly the upper row's height (so the offsets really drive
      geometry, not a uniform pitch).
  (C) the window slides — a real `scene/wheel` then a precise
      `scene/scroll`; the band shifts to matching higher indices and stays
      small; scroll-to-bottom reaches the last row and the offset clamps to
      `total_height - viewport` (proving the sizer drove `max_y`).
  (D) scrollbar peer — the thumb is tiny (sized against the total extent).

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render):
      the four height tiers paint as four distinct surface tones, and the
      list region is tonally distinct from the page surround.
"""

from __future__ import annotations

import bisect
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
    wait_snap,
    sample_png_points,
)

EXAMPLE = "hello-variable-list"
WIN = (360, 520)
N = 10_000
HEIGHTS = [28, 44, 60, 76]
VP_H = 360
OVERSCAN = 3
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"
BAR_TAG = "vlist_scrollbar"


def row_height(i: int) -> int:
    return HEIGHTS[i % len(HEIGHTS)]


def build_offsets(n: int) -> list[int]:
    """Python mirror of `RowOffsets::from_heights` — cumulative tops."""
    offs = [0]
    acc = 0
    for i in range(n):
        acc += row_height(i)
        offs.append(acc)
    return offs


OFFSETS = build_offsets(N)
TOTAL_H = OFFSETS[-1]  # 2500 cycles * 208 = 520_000


def visible_window(offset: int, vp_h: int, offs: list[int], overscan: int) -> set[int]:
    """Python mirror of `compute_visible_range_variable` — the variable
    windowing SSOT. `bisect_right(offs, x)` == Rust `partition_point(t <= x)`
    (count of cumulative tops <= x); subtracting one gives the row
    containing pixel x, mirroring `floor(x/pitch)` in the uniform path."""
    n = len(offs) - 1
    if n == 0 or vp_h == 0 or offs[-1] == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + vp_h
    max_index = n - 1
    first_visible = min(bisect.bisect_right(offs, offset) - 1, max_index)
    last_visible = min(bisect.bisect_right(offs, bottom - 1) - 1, max_index)
    first = max(0, first_visible - overscan)
    last = min(last_visible + overscan, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
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

        # ── (A) boot window: a small window of 10,000, matching math ──
        rects = unclipped_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        rows = present_rows(snap)
        expected = visible_window(0, VP_H, OFFSETS, OVERSCAN)
        assert_eq(rows, expected, "boot rendered band == variable windowing math")
        assert len(rows) < 40, f"virtualized: small window, got {len(rows)} of {N}"
        assert 0 in rows, "row 0 rendered at top"
        assert 5000 not in rows, "a deep row is NOT rendered (the whole point)"
        assert 9999 not in rows, "the last row is NOT rendered at the top"

        # ── (B) rows sized + positioned by the prefix-sum table ──────
        # Each rendered row's painted height equals its modeled height.
        for i in sorted(rows):
            assert_eq(rects[f"vlist#{i}"][3], row_height(i),
                      f"row {i} painted height == modeled height {row_height(i)}")
        # Heights actually vary (not a uniform pitch in disguise).
        seen_heights = {rects[f"vlist#{i}"][3] for i in rows}
        assert len(seen_heights) >= 3, \
            f"rows show multiple distinct heights, got {sorted(seen_heights)}"
        # Adjacent row tops differ by exactly the upper row's height.
        for i in sorted(rows)[:-1]:
            if i + 1 in rows:
                dy = rects[f"vlist#{i + 1}"][1] - rects[f"vlist#{i}"][1]
                assert_eq(dy, row_height(i),
                          f"row {i}->{i+1} top delta == row {i} height (offsets drive y)")

        # ── (D) scrollbar thumb is tiny (bound = total extent) ───────
        bar = find_by_tag(snap, BAR_TAG)
        assert bar is not None, "scrollbar peer present"
        thumb = (bar.get("children") or [None])[0]
        assert isinstance(thumb, dict), "scrollbar has a thumb child"
        thumb_h = int(thumb["rect"]["h"])
        assert thumb_h < VP_H // 8, \
            f"thumb tiny vs viewport (sized against {TOTAL_H}px total), got {thumb_h}"

        # ── (C) the window slides — real wheel input first ───────────
        tf.wheel(path=SCROLL_TAG, pixels=(0.0, 2000.0))
        snap = wait_snap(
            tf, lambda s: scroll_offset(s) > 0, viewport=WIN,
            desc="wheel advanced the offset",
        )
        off1 = scroll_offset(snap)
        rows1 = present_rows(snap)
        assert 0 not in rows1, "top row scrolled out of the window after wheel"
        assert_eq(rows1, visible_window(off1, VP_H, OFFSETS, OVERSCAN),
                  "post-wheel band == variable windowing math at the new offset")
        assert len(rows1) < 40, f"window stays small after wheel, got {len(rows1)}"

        # ── (C) precise programmatic scroll to a deep offset ─────────
        tf.scroll(SCROLL_TAG, to=(0, 100_000))
        snap = wait_snap(
            tf, lambda s: scroll_offset(s) == 100_000, viewport=WIN,
            desc="scroll-to landed at offset 100000",
        )
        rows2 = present_rows(snap)
        expected2 = visible_window(100_000, VP_H, OFFSETS, OVERSCAN)
        assert_eq(rows2, expected2, "deep band == variable windowing math")
        assert 0 not in rows2 and 9999 not in rows2, "only the mid band exists"
        assert len(rows2) < 40, f"window still small deep in the list, got {len(rows2)}"
        # Heights still tracked deep in the list.
        for i in sorted(rows2):
            assert_eq(rects_h(snap, i), row_height(i),
                      f"deep row {i} painted height == modeled {row_height(i)}")

        # ── (C) scroll to the very bottom — last row + clamped max ───
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # past the end → clamps
        max_off = TOTAL_H - VP_H  # 520000 - 360 = 519640
        snap = wait_snap(
            tf, lambda s: scroll_offset(s) == max_off, viewport=WIN,
            desc="offset clamps to total_height - viewport (sizer drove max_y)",
        )
        rows3 = present_rows(snap)
        assert 9999 in rows3, "the last row is reachable at the bottom"
        assert_eq(rows3, visible_window(max_off, VP_H, OFFSETS, OVERSCAN),
                  "bottom band == variable windowing math")
        assert len(rows3) < 40, f"window small even at the bottom, got {len(rows3)}"

        # ── scroll back to top restores the boot window ──────────────
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(scroll_offset(snap), 0, "scrolled back to top")
        assert_eq(present_rows(snap), expected, "top window restored exactly")

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Four height tiers carry four distinct surface tones, so the windowed
    # rows really painted (an empty clip would be one flat colour). Sample
    # each tier's right interior (past the left-aligned label, no glyphs).
    def row_bg_point(i: int):
        x, y, w, h = rects[f"vlist#{i}"]
        return (x + w - 8, y + h // 2)

    tones = sample_png_points(img, [row_bg_point(i) for i in range(4)])
    assert len(set(tones)) == 4, \
        f"four height tiers paint four distinct tones, got {tones}"

    # The list region is tonally distinct from the page surround.
    lx, ly, lw, lh = rects[LIST_TAG]
    if lx >= 12:
        page_pt = (max(2, lx - 8), ly + lh // 2)
        page = sample_png_points(img, [page_pt])[0]
        assert page not in tones, \
            f"list rows are tonally distinct from the page surround, got {page} vs {tones}"


def rects_h(snap, i: int) -> int:
    return unclipped_rects_of(snap)[f"vlist#{i}"][3]


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, unclipped_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r745-")) / "vlist.png"
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
    sys.exit(run_demo("R745 §5.27 — Model/View variable-height virtualization", body))
