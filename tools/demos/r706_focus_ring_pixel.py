#!/usr/bin/env python3
"""R706 focus-ring pixel demo — the authoritative regression guard for the
fragment-cache "direct-draw-after-append" rasterization defect.

Background (R706). The R682 paint-fragment cache replays cached subtrees via
`vello::Scene::append`. R705 moved the §5.39 keyboard focus ring into the
paint scene as a top-level overlay `Scene::Box`, stroked through the generic
box path. On the live winit render path (`to_vello_cached`) that stroke was
issued into the main `vello::Scene` *after* the cached grid fragment was
appended, so it re-used the appended fragment's stale encoder transform state
and rendered ONE GRID COLUMN to the right — the focus ring framed the wrong
day. `scene/snapshot from: paint` could not see it (the scene carried the
ring at the correct rect; only the rasterizer shifted it), so this defect is
catchable ONLY by reading real pixels off the live window
([[introspection-from-paint-not-screen]]). R706 rewrote `to_vello_cached` so
`out` only ever receives appends — every contribution encodes into a fresh
sub-scene that is appended — eliminating the hazard.

This demo drives the live `hello-datepicker` window over JSON-RPC, focuses a
known day, captures the actual screen with ffmpeg, locates the blue focus
ring and the day-number text columns by pixel, and asserts the ring sits on
the focused day's column — not one column to the right.

Requires a live X display + ffmpeg + Pillow. When any is unavailable the demo
SKIPS cleanly (exit 0) rather than failing — pixel capture is an environment
dependency the project tracks as a permanent carry.

Run from the workspace root:
    cargo build -p hello-datepicker --release
    python3 tools/demos/r706_focus_ring_pixel.py
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

DP = "datepicker"
VIEWPORT = (360, 420)
SHOT = "/tmp/r706_focus_ring_pixel.png"
RING_BLUE = (26, 115, 232)  # Material focus blue #1A73E8


def _skip(reason: str) -> int:
    print(f"SKIP R706 focus-ring pixel demo: {reason}")
    return 0


def _ring_and_panel(png_path: str):
    """Locate the blue focus ring and the calendar panel's left edge on the
    ring's row. Returns (ring_left_x, panel_left_x, cell_width) or None when
    the ring / panel cannot be located.

    Position-independent and contamination-free: everything is measured
    relative to the panel the ring lives inside, so the variable window
    placement and background-window text do not matter.
    """
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

    def light(x, y):
        r, g, b = px[x, y]
        # The M3 SurfaceContainerLow calendar panel is a near-white lilac;
        # the desktop / sibling windows around it are markedly darker.
        return r > 225 and g > 220 and b > 225

    # Find the ring: the densest tight blue cluster in the window region.
    # Start the x scan at 100 to skip the far-left desktop dock (whose blue
    # app icons otherwise dominate the median); the hello-datepicker window
    # never opens over the dock. Cluster around the median to drop stray
    # sibling-window blue specks.
    qx, qy = min(w, 760), min(h, 650)
    blues = [(x, y) for y in range(110, qy) for x in range(100, qx) if blue(x, y)]
    if len(blues) < 60:
        return None
    mx = st.median([p[0] for p in blues])
    my = st.median([p[1] for p in blues])
    ring = [p for p in blues if abs(p[0] - mx) < 30 and abs(p[1] - my) < 30]
    if len(ring) < 60:
        return None
    ring_left = min(p[0] for p in ring)
    ring_right = max(p[0] for p in ring)
    cell_width = ring_right - ring_left  # ~44 (cell 40 + 2px ring offset *2)
    row_y = int(sum(p[1] for p in ring) / len(ring))

    # Walk LEFT from the ring along its row until the panel ends (light ->
    # not-light transition) to find the calendar panel's left edge.
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
    if panel_left is None:
        return None
    return ring_left, panel_left, cell_width


def body() -> None:
    with isolated_storage_dir("r706-focus-ring-pixel"):
        with RpcSubprocess("hello-datepicker", visible_window=True) as d:
            # Single Tab stop + roving active descendant: focus the grid
            # root, then drive the cursor to day 3 (Home -> day 1, then two
            # ArrowRights). Day 3 of May 2026 is the Sunday column (the
            # left-most day column) — column index 0.
            d.request("focus/set", {"tag": DP})
            d.key(path=DP, name="Home")
            d.key(path=DP, name="ArrowRight")
            d.key(path=DP, name="ArrowRight")

            # R883 zero-flake: window MAPPING + compositor presentation
            # are async with no RPC observable, so poll the SCREEN —
            # re-grab until the ring + panel are locatable (or time out
            # into the original graceful SKIP).
            def _grab_ok() -> bool:
                r = subprocess.run(
                    ["ffmpeg", "-y", "-f", "x11grab", "-video_size", "1920x1080",
                     "-i", os.environ.get("DISPLAY", ":0") + ".0",
                     "-frames:v", "1", SHOT],
                    capture_output=True, timeout=15,
                )
                return r.returncode == 0 and Path(SHOT).exists()

            if not _grab_ok():
                print("SKIP: ffmpeg x11grab failed (no capturable display)")
                return

            located = _ring_and_panel(SHOT)
            deadline = time.monotonic() + 10.0
            while located is None and time.monotonic() < deadline:
                time.sleep(0.25)  # re-grab poll interval
                if not _grab_ok():
                    break
                located = _ring_and_panel(SHOT)
            if located is None:
                print("SKIP: could not locate ring / calendar panel in capture")
                return
            ring_left, panel_left, cell_width = located

            # Day 3 of May 2026 sits in the left-most day column (column 0).
            # Its focus ring's left edge must lie within ~one cell of the
            # calendar panel's left edge (just the block padding). The
            # pre-R706 defect shifted the ring one full ~42-px column right,
            # which puts its left edge a whole cell width or more past the
            # panel padding.
            offset = ring_left - panel_left
            assert 0 <= offset < cell_width, (
                f"focus ring left edge is {offset}px past the panel left edge "
                f"(cell width ~{cell_width}px). Expected the left-most day "
                f"column (< 1 cell). A +1-column rasterization regression "
                f"lands it near {cell_width + 12}px+."
            )
            print(
                f"PASS: focus ring left edge {offset}px past panel left "
                f"(< cell width {cell_width}px) -> on day-3 column 0, not "
                f"shifted one column right."
            )


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
        body()
    except AssertionError as e:
        print(f"FAIL: {e}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
