#!/usr/bin/env python3
"""R806 focus-ring corner demo — concentric ring at the top/left framebuffer edge.

Background (R806). The §5.39 keyboard focus ring (R705) is a top-level
overlay `Scene::Box`: the focused widget's rect inflated outward by
`FocusRingStyle::offset` (2px) on every side. `Rect` origins are unsigned,
so a widget flush against the top/left framebuffer edge (`x` or `y` < offset)
cannot carry the negative origin a full outward offset needs. The pre-R806
`build_focus_ring_box` used a naive `saturating_sub` on the origin while
keeping the full `+2*offset` span, so for a top-left-flush widget the
bottom/right edge was pushed an extra `offset` px out — a lopsided, oversized
ring. A `hello-menu` menubar title at `(0,0,96,40)` got a `(0,0,100,44)` ring
(doubled bottom/right gap, none at top/left) instead of the concentric
`(0,0,98,42)`. This was the user-reported menubar focus-ring corner defect.

The fix clamps the near origin to 0 AND shrinks the span by the same clamped
amount, so the far edge stays at `target + offset` — the ring is concentric
with the widget and the framebuffer edge clips the lost top/left gap (exactly
how a browser clips a viewport-corner focus ring). The ring *stroke* itself
was always a faithful 2px on every edge; the defect was the non-concentric
*placement*, observable only as the bottom/right edge sitting 2px too far out
(scene-data) and, on screen, the ring crammed into the corner with an
asymmetric gap ([[introspection-from-paint-not-screen]]: the stroke width
matched intent, the placement did not).

This demo drives the live `hello-menu` window over JSON-RPC. It focuses the
menubar and arrows across all three top-flush titles, asserting each ring is
the concentric boundary-clipped inflate of its title via scene-data (the
deterministic core, always run), then — when a live X display + ffmpeg are
present — captures the actual window and asserts the `File` ring renders a
uniform 2px stroke on every edge framed concentrically at the corner.

Run from the workspace root:
    cargo build -p hello-menu --release
    python3 tools/demos/r806_focus_ring_corner.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    FOCUS_RING_TAG,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_focus_ring_concentric,
    isolated_storage_dir,
    run_demo,
)

BAR = "menu"
VIEWPORT = (520, 320)
OFFSET = 2  # FocusRingStyle::default().offset
TOP_EDGE_INSET = 1  # build_focus_ring_box: top stroke kept off the vello y=0 flood row
SHOT = "/tmp/r806_focus_ring_corner.png"
RING_BLUE = (26, 115, 232)  # Material focus blue #1A73E8

# Expected window-absolute geometry of the three top-flush menubar titles
# (hello-menu MENU_TITLES, 96px wide / 40px tall, packed from x=0 at y=0) and
# the concentric ring each must carry. The TOP origin floors at
# TOP_EDGE_INSET (1) — a stroke touching the framebuffer y=0 row is flooded
# ~16px thick by vello — while the LEFT origin floors at 0 (no left flood).
#   File t0: flush top-left -> x floors 0, y floors 1 -> (0,1,98,41)
#   Edit t1: flush top only -> x = 96-2,  y floors 1 -> (94,1,100,41)
#   View t2: flush top only -> x = 192-2, y floors 1 -> (190,1,100,41)
TITLES = [
    ("menu#t0", (0, 0, 96, 40), (0, 1, 98, 41)),
    ("menu#t1", (96, 0, 96, 40), (94, 1, 100, 41)),
    ("menu#t2", (192, 0, 96, 40), (190, 1, 100, 41)),
]


def _concentric_clip(r: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
    """Reference concentric boundary-clip inflate (mirrors the Rust fix):
    left origin floors at 0, top origin floors at TOP_EDGE_INSET."""
    x = max(0, r[0] - OFFSET)
    y = max(TOP_EDGE_INSET, r[1] - OFFSET)
    return (x, y, r[0] + r[2] + OFFSET - x, r[1] + r[3] + OFFSET - y)


def _check_title(app: RpcSubprocess, bar_focus: int) -> None:
    """Assert the ring on title `bar_focus` is the concentric clipped inflate
    of that title, with the boundary clamp applied per axis."""
    tag, want_title, want_ring = TITLES[bar_focus]
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    rects = abs_rects_of(snap)

    # The title paints exactly where we expect (guards against a layout drift
    # silently changing what "flush at the edge" means).
    assert_eq(rects.get(tag), want_title, f"{tag} window-absolute rect")

    ring = rects.get(FOCUS_RING_TAG)
    assert ring is not None, f"ring present while title {bar_focus} focused"
    assert_eq(ring, want_ring, f"{tag} concentric clipped ring rect")

    # Reference formula agreement (the helper mirror + the local mirror both
    # reproduce the Rust geometry).
    assert_eq(ring, _concentric_clip(want_title), f"{tag} ring == reference inflate")
    framed = assert_focus_ring_concentric(snap, offset=OFFSET)
    assert_eq(framed, tag, f"helper frames {tag}")

    # Per-axis clamp + concentric far edges. The top floors at the inset
    # (off the vello y=0 flood row), the left floors at 0.
    assert_eq(ring[1], TOP_EDGE_INSET, f"{tag} top stroke kept off the y=0 flood row")
    assert_eq(ring[1] + ring[3], want_title[1] + want_title[3] + OFFSET,
              f"{tag} bottom edge concentric (widget bottom + offset)")
    assert_eq(ring[0] + ring[2], want_title[0] + want_title[2] + OFFSET,
              f"{tag} right edge concentric (widget right + offset)")
    if want_title[0] < OFFSET:
        assert_eq(ring[0], 0, f"{tag} left edge clamped to framebuffer (x=0)")
    else:
        assert_eq(ring[0], want_title[0] - OFFSET,
                  f"{tag} left edge keeps full outset (not flush on x)")

    # Pre-R806 regression guard: the naive saturating-inflate would have kept
    # the full +2*offset span off a clamped origin, pushing the far edge an
    # extra offset out.
    naive = (max(0, want_title[0] - OFFSET), max(0, want_title[1] - OFFSET),
             want_title[2] + 2 * OFFSET, want_title[3] + 2 * OFFSET)
    if want_title[0] < OFFSET or want_title[1] < OFFSET:
        assert ring != naive, (
            f"{tag} ring must not be the pre-R806 oversized inflate {naive}"
        )


def _pixel_check_file_ring() -> bool:
    """Capture the live window and assert the `File` ring renders a uniform
    2px stroke on every edge, framed concentrically at the top-left corner.
    Returns True when the check ran, False when it skipped (no display)."""
    if not os.environ.get("DISPLAY") or shutil.which("ffmpeg") is None:
        return False
    try:
        from PIL import Image  # noqa: F401
    except ImportError:
        return False

    import re

    tree = subprocess.run(["xwininfo", "-root", "-tree"],
                          capture_output=True, text=True).stdout
    geom = None
    for line in tree.splitlines():
        if "hello-menu" in line:
            m = re.search(r"\d+x\d+\+-?\d+\+-?\d+\s+\+(-?\d+)\+(-?\d+)", line)
            if m:
                geom = (int(m.group(1)), int(m.group(2)))
            break
    if geom is None:
        return False
    ax, ay = geom

    r = subprocess.run(
        ["ffmpeg", "-y", "-f", "x11grab", "-video_size", "1920x1080",
         "-i", os.environ.get("DISPLAY", ":0") + ".0", "-frames:v", "1", SHOT],
        capture_output=True, timeout=15)
    if r.returncode != 0 or not Path(SHOT).exists():
        return False

    from PIL import Image
    im = Image.open(SHOT).convert("RGB")
    px = im.load()
    W, H = im.size

    def blue(x, y):
        if not (0 <= x < W and 0 <= y < H):
            return False
        rr, gg, bb = px[x, y]
        return (abs(rr - RING_BLUE[0]) <= 40 and abs(gg - RING_BLUE[1]) <= 40
                and abs(bb - RING_BLUE[2]) <= 40)

    blues = [(x, y) for y in range(ay, min(ay + 80, H))
             for x in range(ax, min(ax + 130, W)) if blue(x, y)]
    assert len(blues) > 40, f"File ring not located on screen ({len(blues)} px)"
    minx = min(p[0] for p in blues)
    maxx = max(p[0] for p in blues)
    miny = min(p[1] for p in blues)
    maxy = max(p[1] for p in blues)

    # Concentric ring is 98x41 (top inset 1); allow +/-3 for AA fringe.
    assert abs((maxx - minx) - 98) <= 3, f"ring width {maxx - minx} != ~98"
    assert abs((maxy - miny) - 41) <= 3, f"ring height {maxy - miny} != ~41"

    cx = (minx + maxx) // 2
    cy = (miny + maxy) // 2

    def run_len(x0, y0, dx, dy):
        n = 0
        x, y = x0, y0
        while blue(x, y):
            n += 1
            x += dx
            y += dy
        return n

    top = run_len(cx, miny, 0, 1)
    bottom = run_len(cx, maxy, 0, -1)
    left = run_len(minx, cy, 1, 0)
    right = run_len(maxx, cy, -1, 0)
    for edge, th in (("top", top), ("bottom", bottom),
                     ("left", left), ("right", right)):
        assert 1 <= th <= 4, (
            f"File ring {edge} edge renders {th}px — expected a uniform ~2px "
            f"stroke (the corner defect was placement, never a fat stroke)"
        )
    print(f"[pixel] File ring {maxx - minx}x{maxy - miny} on screen, "
          f"edges top={top} bottom={bottom} left={left} right={right} px")
    return True


def body() -> None:
    with isolated_storage_dir("r806-focus-ring-corner"):
        with RpcSubprocess("hello-menu", boot_grace=1.0) as app:
            # Focus the menubar: closed + focused -> ring on the bar_focus
            # title (File, the top-LEFT-flush corner case).
            app.request("focus/set", {"tag": BAR})
            for _ in range(3):
                app.snapshot(source="paint", viewport=VIEWPORT)
                time.sleep(0.1)
            _check_title(app, 0)

            # Pixel verification of the File corner ring while it is focused
            # and CLOSED (ring on the title, not a dropdown item). Run here,
            # before the arrow sweep opens any menu.
            ran_pixels = _pixel_check_file_ring()

            # Arrow across the remaining top-flush titles (top clamp only).
            app.key(path=BAR, name="ArrowRight")
            time.sleep(0.1)
            _check_title(app, 1)

            app.key(path=BAR, name="ArrowRight")
            time.sleep(0.1)
            _check_title(app, 2)

            # Control: a non-flush focused node keeps the full symmetric
            # outset. Open File (ArrowLeft back to t0, ArrowDown opens it),
            # whose active item paints in the dropdown below the bar (y well
            # clear of the top edge), so its ring is NOT top-clamped.
            app.key(path=BAR, name="ArrowLeft")  # t2 -> wrap to t0 (File)
            app.key(path=BAR, name="ArrowDown")  # open File, active item 0
            time.sleep(0.15)
            snap = app.snapshot(source="paint", viewport=VIEWPORT)
            rects = abs_rects_of(snap)
            item = rects.get("menu#i0")
            assert item is not None, "open File: active item 0 paints"
            assert item[1] >= OFFSET, "dropdown item is clear of the top edge"
            framed = assert_focus_ring_concentric(snap, offset=OFFSET)
            assert_eq(framed, "menu#i0", "ring frames the active dropdown item")
            ring = rects.get(FOCUS_RING_TAG)
            assert_eq(ring[1], item[1] - OFFSET,
                      "non-flush item keeps full top outset (no clamp)")
            assert_eq(ring[3], item[3] + 2 * OFFSET,
                      "non-flush item ring keeps full symmetric height")

            print("[demo] pixel check "
                  + ("ran" if ran_pixels else "skipped (no display/ffmpeg/PIL)"))


def main() -> int:
    return run_demo("r806_focus_ring_corner", body)


if __name__ == "__main__":
    sys.exit(main())
