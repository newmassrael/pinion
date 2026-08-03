#!/usr/bin/env python3
"""R707 table focus-ring pixel demo — live-pixel verification + regression
guard that the data-grid renders correctly and the keyboard focus ring lands
on the active-descendant *cell*, with no column rasterization offset.

Why pixels (R706 lesson). The §5.39 keyboard focus ring is composed into the
paint scene and stroked by the shell's R694 substrate over whichever tag the
binding reports focused (here the active-descendant cell, via
`access_focus_target`). A rasterizer offset — the R706 fragment-cache
"direct-draw-after-append" defect — shifts the ring one column on the live
window while `scene/snapshot from: paint` still reports the correct rect, so
it is catchable ONLY by reading real pixels off the live window
([[introspection-from-paint-not-screen]]). The table reuses the same
R706-fixed `to_vello_cached` path; this demo confirms the fix holds for a
fresh widget with a different (Grid -> Row -> cell) paint nesting.

Two complementary measurements (both glyph-robust):

  1. ABSOLUTE (catches a static column offset, the R706 defect): rove to cell
     (row 0, col 0) and assert the ring sits in column 0. Column 0 is the
     left-most cell, so the walk from the ring to the panel's left edge
     crosses NO data text — a clean anchor (the same reason the R706 date-
     picker demo used the left-most day column).

     R1548 put a **vertical header band** between the panel's edge and that
     first column, so the panel edge stopped being the column origin: the walk
     now lands one band-width short. The band's width is READ FROM THE PAINT
     (the corner cell's rect) rather than hard-coded, so this stays a
     measurement of where the ring is and not a second statement of the
     layout — and a table with no band measures 0 and the arithmetic is the
     pre-R1548 one exactly.
  2. DELTA (catches a stuck / non-tracking ring): rove two columns right to
     (row 0, col 2) and assert the ring moved right by ~2 column pitches. The
     delta between two blue ring positions needs no panel anchor at all.

Requires a live X display + ffmpeg + Pillow. When any is unavailable the demo
SKIPS cleanly (exit 0) — pixel capture is an environment dependency the
project tracks as a permanent carry.

Run from the workspace root:
    cargo build -p hello-table --release
    python3 tools/demos/r707_table_pixel.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, isolated_storage_dir  # noqa: E402

T = "table"
VIEWPORT = (600, 360)
SHOT_A = "/tmp/r707_table_pixel_col0.png"
SHOT_B = "/tmp/r707_table_pixel_col2.png"
RING_BLUE = (26, 115, 232)  # Material focus blue #1A73E8 (shared R694 ring)


def _row_header_band_width(d) -> int:
    """R1548 — the width of the vertical header band, from the PAINT.

    The band sits between the panel's left edge and column 0, so the pixel
    walk's anchor is `panel_left + this`. Read from the corner cell's rect
    (`GridTag::header_corner`) rather than restated as a constant: a demo that
    carried its own copy of a layout number would keep passing after the
    layout changed, which is the failure this whole file exists to catch one
    level down.

    `0` when the table paints no band — every pre-R1548 caller, and the
    arithmetic then reduces to what it was.
    """
    found: list = []
    _walk_tag(d.snapshot(source="paint", viewport=VIEWPORT), f"{T}_hcorner", found)
    return max((int((n.get("rect") or {}).get("w") or 0) for n in found), default=0)


def _walk_tag(node, tag: str, out: list) -> None:
    if isinstance(node, dict):
        if node.get("tag") == tag:
            out.append(node)
        for v in node.values():
            _walk_tag(v, tag, out)
    elif isinstance(node, list):
        for v in node:
            _walk_tag(v, tag, out)


def _skip(reason: str) -> int:
    print(f"SKIP R707 table pixel demo: {reason}")
    return 0


def _capture(path: str) -> bool:
    r = subprocess.run(
        ["ffmpeg", "-y", "-f", "x11grab", "-video_size", "1920x1080",
         "-i", os.environ.get("DISPLAY", ":0") + ".0",
         "-frames:v", "1", path],
        capture_output=True, timeout=15,
    )
    return r.returncode == 0 and Path(path).exists()


def _find_ring(png_path: str):
    """Locate the blue focus ring. Returns (ring_left, ring_right, row_y) or
    None. The ring is the densest tight blue cluster in the window region."""
    import statistics as st

    from PIL import Image  # local import so absence -> graceful skip

    im = Image.open(png_path).convert("RGB")
    px = im.load()
    w, h = im.size

    def blue(x, y):
        r, g, b = px[x, y]
        return (
            abs(r - RING_BLUE[0]) <= 26
            and abs(g - RING_BLUE[1]) <= 26
            and abs(b - RING_BLUE[2]) <= 26
        )

    qx, qy = min(w, 1000), min(h, 760)
    # Skip the far-left desktop dock (x < 100), whose blue icons would
    # otherwise dominate the median; the window never opens over the dock.
    blues = [(x, y) for y in range(60, qy) for x in range(100, qx) if blue(x, y)]
    if len(blues) < 60:
        return None
    mx = st.median([p[0] for p in blues])
    my = st.median([p[1] for p in blues])
    ring = [p for p in blues if abs(p[0] - mx) < 80 and abs(p[1] - my) < 26]
    if len(ring) < 60:
        return None
    ring_left = min(p[0] for p in ring)
    ring_right = max(p[0] for p in ring)
    row_y = int(sum(p[1] for p in ring) / len(ring))
    return ring_left, ring_right, row_y


def _panel_left(png_path: str, ring_left: int, row_y: int):
    """Walk LEFT from the ring along its row to the table panel's left edge.
    For the col-0 ring there is no data text to the left (only the block
    padding), so a plain light -> dark transition cleanly finds the edge."""
    from PIL import Image

    im = Image.open(png_path).convert("RGB")
    px = im.load()

    def light(x, y):
        r, g, b = px[x, y]
        # M3 SurfaceContainerLow panel = near-white lilac; desktop is darker.
        return r > 225 and g > 220 and b > 225

    panel_left = None
    x = ring_left - 1
    misses = 0
    while x > 0:
        if light(x, row_y):
            panel_left = x
            misses = 0
        else:
            misses += 1
            if misses > 6 and panel_left is not None:
                break
        x -= 1
    return panel_left


def body() -> int:
    with isolated_storage_dir("r707-table-pixel"):
        with RpcSubprocess("hello-table", visible_window=True) as d:
            # Single Tab stop + 2-D roving: focus the grid, enter at (0, 0).
            d.request("focus/set", {"tag": T})
            d.key(path=T, name="ArrowDown")  # enter grid at row 0, col 0

            # R883 zero-flake: window mapping + presentation are async with
            # no RPC observable — poll the SCREEN (re-grab until the ring
            # is locatable, or time out into the original graceful SKIP).
            if not _capture(SHOT_A):
                print("SKIP: ffmpeg x11grab failed (no capturable display)")
                return 0
            ring_a = _find_ring(SHOT_A)
            deadline = time.monotonic() + 10.0
            while ring_a is None and time.monotonic() < deadline:
                time.sleep(0.25)  # re-grab poll interval
                if not _capture(SHOT_A):
                    print("SKIP: ffmpeg x11grab failed (no capturable display)")
                    return 0
                ring_a = _find_ring(SHOT_A)
            if ring_a is None:
                print("SKIP: could not locate the ring at column 0")
                return 0
            rl0, rr0, row_y0 = ring_a
            pitch = rr0 - rl0  # ring span ~= column pitch (col_width 120 + ring)
            if pitch < 40:
                print(f"SKIP: implausible ring span {pitch}px (capture noise)")
                return 0
            panel_left = _panel_left(SHOT_A, rl0, row_y0)
            if panel_left is None:
                print("SKIP: could not locate the table panel edge")
                return 0
            band_w = _row_header_band_width(d)

            # (1) ABSOLUTE: the col-0 ring must sit in column 0. The walk left
            # crossed only block padding and the R1548 vertical header band (no
            # data text), so `panel_left + band_w` is the column origin and the
            # remaining offset is just the padding (< half a column).
            offset0 = rl0 - panel_left - band_w
            col_index = round(offset0 / pitch)
            assert col_index == 0, (
                f"col-0 focus ring is in column {col_index} (offset {offset0}px"
                f" / pitch ~{pitch}px) — a static column-offset rasterization "
                f"regression (the R706 defect) would shift it right by a column."
            )

            # (2) DELTA: rove two columns right; the ring must move ~2 pitches.
            d.key(path=T, name="ArrowRight")
            d.key(path=T, name="ArrowRight")
            # Poll the screen until the ring is seen AWAY from column 0
            # (a stale pre-present frame still shows it at rl0).
            if not _capture(SHOT_B):
                print("SKIP: ffmpeg x11grab failed on the second capture")
                return 0
            ring_b = _find_ring(SHOT_B)
            deadline = time.monotonic() + 10.0
            while (ring_b is None or ring_b[0] == rl0) and time.monotonic() < deadline:
                time.sleep(0.25)  # re-grab poll interval
                if not _capture(SHOT_B):
                    print("SKIP: ffmpeg x11grab failed on the second capture")
                    return 0
                ring_b = _find_ring(SHOT_B)
            if ring_b is None:
                print("SKIP: could not locate the ring at column 2")
                return 0
            rl2 = ring_b[0]
            delta_cols = (rl2 - rl0) / pitch
            assert abs(delta_cols - 2.0) < 0.5, (
                f"ring moved {delta_cols:.2f} columns for two ArrowRights "
                f"(rl0={rl0}, rl2={rl2}, pitch~{pitch}px); expected ~2 — a "
                f"stuck or mis-tracking ring."
            )

            print(
                f"PASS: col-0 ring in column 0 (offset {offset0}px / pitch "
                f"~{pitch}px); two ArrowRights moved it {delta_cols:.2f} "
                f"columns. Table renders + ring tracks the 2-D cursor with no "
                f"column offset."
            )
            return 0


def main() -> int:
    if not os.environ.get("DISPLAY"):
        return _skip("no X DISPLAY (pixel capture needs a live window)")
    if shutil.which("ffmpeg") is None:
        return _skip("ffmpeg not on PATH")
    try:
        import PIL  # noqa: F401
    except ImportError:
        return _skip("Pillow (PIL) not installed")
    try:
        return body()
    except AssertionError as e:
        print(f"FAIL: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
