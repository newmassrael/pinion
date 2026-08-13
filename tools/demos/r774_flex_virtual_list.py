#!/usr/bin/env python3
"""R774 §5.27 — flex-viewport (AutoSizer) virtualization E2E.

Drives the `hello-flex-virtual-list` binding via JSON-RPC. First consumer
of `pinion_widget_paint::virtual_list::view_flex_virtual_list`: a
10,000-row list whose `ScrollNode` flex-grows to fill the window and
windows against the RUNTIME-MEASURED viewport height (react-window
AutoSizer) instead of a caller-supplied const.

The decisive witness is scene-as-data (§2 #7), not pixels: an ascending
sweep of real `scene/resize` calls grows the actual window, and the
rendered row count tracks the runtime-MEASURED viewport at each step. The
flex-computed clip height is reported back on the `Scroll` node's
`viewport`, proving the layout pass measured the container (not a const).
A taller window materializes MORE rows of the same 10,000-row dataset.

Atomic verification scope (>=30 assertions):

  (A) AutoSizer measurement — the `Scroll` clip-window height equals the
      flex-distributed band (window height minus the fixed header), NOT a
      const.
  (B) live `scene/resize` => rendered count tracks the measured viewport
      — across an ascending window-height sweep the `Scroll` clip height
      grows, the present row-index set equals the windowing math fed the
      OBSERVED (measured) height, and the row count strictly increases,
      all far below N = 10,000. The resize-event plumbing end to end
      (winit Resized → relayout → AutoSizer feedback → re-window), not a
      snapshot-viewport param.
  (C) the window still slides — a real `scene/wheel` then a precise
      `scene/scroll` advance the offset; the band shifts to the matching
      higher indices and stays viewport-sized; scroll-to-bottom clamps to
      `N*pitch - measured_viewport` (the sizer drove `max_y`).

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render):
      the windowed rows really paint — adjacent rows show distinct zebra
      tones and the list band is tonally distinct from the header bar.

Note: this WM honours window-GROW resize requests but not shrink, so the
sweep is ascending; the deterministic shrink + zero-height first-paint
warmup behaviour is covered by the binding's unit tests.
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
    wait_until,
)

EXAMPLE = "hello-flex-virtual-list"
WIN = (380, 480)
N = 10_000
PITCH = 32
HEADER_H = 44
OVERSCAN = 3
SCROLL_TAG = "flexlist_scroll"
LIST_TAG = "flexlist"
BAR_TAG = "flexlist_scrollbar"


def measured_h(win_h: int) -> int:
    """The flex band height the list windows against: the window height
    minus the fixed header bar."""
    return win_h - HEADER_H


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int):
    """Python mirror of `pinion_core::widgets::virtual_list::
    compute_visible_range` — the windowing SSOT. The RPC-observed row
    band is asserted equal to this, fed the MEASURED height."""
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
    for tag in unclipped_rects_of(snap):
        if tag.startswith("flexlist#"):
            out.add(int(tag.split("#", 1)[1]))
    return out


def scroll_node(snap):
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return node


def scroll_offset(snap) -> int:
    return int(scroll_node(snap).get("offset_y", -1))


def scroll_viewport_h(snap) -> int:
    vp = scroll_node(snap).get("viewport") or {}
    return int(vp.get("h", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def resize_to(win_h: int):
            """Drive a real `scene/resize`, then poll `from=paint` until
            the stored frame reflects the new flex-measured clip height
            (the resize event + repaint settled). Returns the snapshot."""
            resp = tf.request("scene/resize", {"width": WIN[0], "height": win_h})
            assert resp is not None and resp.result is not None, "scene/resize accepted"
            want = measured_h(win_h)

            def settled():
                s = snap_now()
                return s if scroll_viewport_h(s) == want else None

            return wait_until(
                settled,
                desc=f"Scroll clip height settles at {want} after resize to {win_h}",
            )

        # ── (A) AutoSizer measurement at the boot window ────────────
        snap = snap_now()
        rects = unclipped_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert SCROLL_TAG in rects, "scroll container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        boot_vp = scroll_viewport_h(snap)
        assert_eq(boot_vp, measured_h(WIN[1]),
                  "Scroll clip height == flex band (window - header), measured not const")

        rows = present_rows(snap)
        expected = visible_window(0, boot_vp, N, PITCH, OVERSCAN)
        assert_eq(rows, expected, "boot band == windowing math fed the MEASURED height")
        assert len(rows) < 30, f"virtualized: small window, got {len(rows)} of {N}"
        assert len(rows) >= 12, f"window covers the visible band, got {len(rows)}"
        assert 0 in rows, "row 0 rendered at top"
        assert 5000 not in rows, "a deep row is NOT rendered (the whole point)"
        assert 9999 not in rows, "the last row is NOT rendered at the top"

        # Rows are vertically ordered, one pitch apart.
        y0 = rects["flexlist#0"][1]
        y1 = rects["flexlist#1"][1]
        assert_eq(y1 - y0, PITCH, "adjacent rows are exactly one pitch apart")

        # ── (B) live resize => rendered count tracks measured viewport
        # An ascending sweep of real `scene/resize` calls (winit Resized →
        # relayout → the flex list measures a bigger clip each step). The
        # rendered row count must strictly increase and equal the windowing
        # math fed the OBSERVED (measured, flex-computed) height — never a
        # const. This is the whole AutoSizer claim, end to end.
        prev_vp = boot_vp
        prev_count = len(rows)
        for win_h in (580, 700, 860):
            snap_g = resize_to(win_h)
            vp_g = scroll_viewport_h(snap_g)
            assert_eq(vp_g, measured_h(win_h), f"resize {win_h}: clip == window - header")
            assert vp_g > prev_vp, f"clip grew: {vp_g} > {prev_vp}"
            rows_g = present_rows(snap_g)
            assert_eq(rows_g, visible_window(0, vp_g, N, PITCH, OVERSCAN),
                      f"resize {win_h}: band == windowing math at the MEASURED height")
            count_g = len(rows_g)
            assert count_g > prev_count, \
                f"taller viewport renders MORE rows: {count_g} > {prev_count} (h={win_h})"
            assert count_g < 40, f"still a window, not the {N}-row dataset: {count_g}"
            assert 0 in rows_g, "row 0 still at the top of the grown window"
            assert 9999 not in rows_g, "the last row is still NOT rendered at the top"
            prev_vp, prev_count = vp_g, count_g

        # The window is now at its tallest (clip 816). Slides work at any
        # measured size — exercise them here rather than shrinking back
        # (this WM honours grow but not shrink requests).
        big_vp = prev_vp

        # ── (C) the window still slides at the (grown) viewport ─────
        tf.wheel(path=SCROLL_TAG, pixels=(0.0, 2000.0))
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) > 0 else None)(snap_now()),
            desc="wheel advanced the offset",
        )
        off1 = scroll_offset(snap)
        assert_eq(scroll_viewport_h(snap), big_vp, "clip unchanged by scrolling")
        rows1 = present_rows(snap)
        assert 0 not in rows1, "top row scrolled out after wheel"
        assert_eq(rows1, visible_window(off1, big_vp, N, PITCH, OVERSCAN),
                  "post-wheel band == windowing math at the new offset")
        assert len(rows1) < 40, f"window stays small after wheel, got {len(rows1)}"

        tf.scroll(SCROLL_TAG, to=(0, 4000))
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == 4000 else None)(snap_now()),
            desc="scroll-to landed at offset 4000",
        )
        rows2 = present_rows(snap)
        assert_eq(rows2, visible_window(4000, big_vp, N, PITCH, OVERSCAN),
                  "deep band == windowing math")
        assert 0 not in rows2 and 9999 not in rows2, "only the mid band exists"
        assert len(rows2) < 40, f"window still small deep in the list, got {len(rows2)}"

        # Scroll to the very bottom — clamps to N*pitch - measured_viewport.
        tf.scroll(SCROLL_TAG, to=(0, 10**9))
        max_off = N * PITCH - big_vp
        snap = wait_until(
            lambda: (lambda s: s if scroll_offset(s) == max_off else None)(snap_now()),
            desc="offset clamps to N*pitch - measured_viewport (sizer drove max_y)",
        )
        rows3 = present_rows(snap)
        assert 9999 in rows3, "the last row is reachable at the bottom"
        assert len(rows3) < 40, f"window small even at the bottom, got {len(rows3)}"

        tf.scroll(SCROLL_TAG, to=(0, 0))
        wait_until(
            lambda: scroll_offset(snap_now()) == 0,
            desc="scrolled back to top",
        )

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    def row_bg_point(i: int):
        x, y, w, h = rects[f"flexlist#{i}"]
        return (x + w - 8, y + h // 2)

    bg0, bg1, bg2 = sample_png_points(img, [row_bg_point(0), row_bg_point(1), row_bg_point(2)])
    assert bg0 != bg1, f"row 0 / row 1 zebra tones differ (rows painted), got {bg0} vs {bg1}"
    assert bg0 == bg2, f"even rows share a tone (zebra parity), got {bg0} vs {bg2}"

    # The list band is tonally distinct from the header bar above it.
    hx, hy, hw, hh = rects[LIST_TAG]
    header_pt = (hx + hw // 2, max(2, hy - HEADER_H // 2))
    header = sample_png_points(img, [header_pt])[0]
    assert header != bg0, f"list rows are tonally distinct from the header, got {header} vs {bg0}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, unclipped_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r774-")) / "flexlist.png"
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
    sys.exit(run_demo("R774 §5.27 — flex-viewport (AutoSizer) virtualization", body))
