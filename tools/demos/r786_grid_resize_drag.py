#!/usr/bin/env python3
"""R786 §5.27 — live-drag column resize (the pointer-wire consumer).

R785 landed the per-column `ColumnWidths` model + the AI-first `set_col_width`
RPC path; R786 is the interaction half of the R780 -> R781 model-then-
interaction split: a clicked-and-dragged column border. Each header cell paints
an 8px grabber at its trailing edge (`ghs_ch<col>#resize`); pressing it captures
the pointer (the R51.34 capture lock, the same primitive the splitter and slider
use) and `pointer_move` maps the cursor 1:1 to the column width via the shared
`ColumnResizeExternal` -> `ColumnWidths.set_width`.

Two complementary verification phases:

  PHASE 1 — scene/drag (deterministic, the regression gate).
    The `scene/drag` RPC presses at the grabber, marches the cursor through N
    interpolated frames, and releases — the shell forwards every frame to the
    `InputRouter` under the capture lock, so the receiving handle's
    `pointer_move` arc fires exactly as a real mouse triggers it. The resize is
    observable as scene-as-data (§2 #7): the `ghs_cols` width model updates, the
    rendered cell rects widen / shift, and the R784 horizontal-scroll extent
    grows (resize ∘ h-scroll compose). >=30 assertions.

  PHASE 2 — NATIVE mouse (XTEST) + ffmpeg live pixels.
    `scene/drag` injects at the `InputRouter`, BYPASSING winit. To prove the
    full native path (winit `MouseInput` / `CursorMoved` -> `ShellCore` ->
    `InputRouter` -> capture -> `pointer_move`), this phase drives a REAL X
    pointer with XTest (`XTestFakeMotionEvent` / `XTestFakeButtonEvent`) over
    the live window: press the grabber, drag right, release. The decisive
    witness is that the width model GROWS after a native drag (the RPC path was
    never touched), and ffmpeg before/after confirms the on-screen grid pixels
    actually changed ([[introspection-from-paint-not-screen]]: a paint-readback
    is not a screen capture). Skips cleanly (exit 0 for the phase) when cc /
    ffmpeg / Xtst / a live display is unavailable — a tracked env carry.

Run from the workspace root:
    cargo build -p hello-grid-hscroll --release
    python3 tools/demos/r786_grid_resize_drag.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-grid-hscroll"
WIN = (520, 440)
NCOLS = 8
COL_W = 130
TABLE_TAG = "ghs"
H_SCROLL_TAG = "ghs_hscroll"
COLS_TAG = "ghs_cols"
PAUSE = 0.12


def col_path(slot: str) -> str:
    return f"/{COLS_TAG}/external/{slot}"


def resize_path(col: int, slot: str) -> str:
    # The per-column ColumnResizeExternal is registered at the header-cell tag.
    return f"/{TABLE_TAG}_ch{col}/external/{slot}"


def grabber_tag(col: int) -> str:
    return f"{TABLE_TAG}_ch{col}#resize"


def cell_w(rects, tag: str) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][2]


def cell_x(rects, tag: str) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][0]


def hscroll_max_x(tf) -> int:
    """Scroll the outer horizontal scroll to the clamp; offset_x then reads the
    live extent (total_w - viewport_w, R784)."""
    tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
    time.sleep(PAUSE)
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll present"
    offset = int(node.get("offset_x", -1))
    tf.scroll(H_SCROLL_TAG, to=(0, 0))  # reset
    time.sleep(PAUSE)
    return offset


def snap_rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=WIN))


def drag_grabber(tf, col: int, dx: int) -> None:
    """Drag column `col`'s border grabber horizontally by `dx` logical px
    (the cursor delta maps 1:1 to the column width)."""
    rects = snap_rects(tf)
    gx, gy, gw, gh = rects[grabber_tag(col)]
    frm = (float(gx + gw // 2), float(gy + gh // 2))
    to = (float(gx + gw // 2 + dx), float(gy + gh // 2))
    tf.drag(from_at=frm, to_at=to, steps=10)
    time.sleep(PAUSE)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5, visible_window=True) as tf:
        # ── (A) boot: grabbers + width model + rendered cells ───────
        snap = tf.snapshot(source="paint", viewport=WIN)
        # A grabber is painted at every column's trailing edge.
        for c in range(NCOLS):
            assert find_by_tag(snap, grabber_tag(c)) is not None, \
                f"col {c} resize grabber present at boot"
        # The per-column resize externals introspect their identity + state.
        assert_eq(tf.query(resize_path(2, "col")), 2, "resize external knows its column")
        assert_eq(tf.query(resize_path(2, "dragging")), False, "no drag in flight at boot")
        # Width model seeded uniform.
        assert_eq(tf.query(col_path("cols")), NCOLS, "model column count")
        assert_eq(tf.query(col_path("widths")), ",".join([str(COL_W)] * NCOLS),
                  "seeded uniform widths")
        assert_eq(tf.query(col_path("total")), NCOLS * COL_W, "total = sum of widths")
        min_w = tf.query(col_path("min_width"))
        assert isinstance(min_w, int) and min_w > 0, f"min_width positive, got {min_w}"

        rects = snap_rects(tf)
        for c in range(4):  # first four columns are within the 520px window
            assert_eq(cell_w(rects, f"ghs#0_{c}"), COL_W, f"col {c} cell at seeded width")
        x2_boot = cell_x(rects, "ghs#0_2")
        x3_boot = cell_x(rects, "ghs#0_3")
        assert_eq(x3_boot - x2_boot, COL_W, "col-3 starts one column-width right of col-2")
        max_x_boot = hscroll_max_x(tf)
        assert max_x_boot > 0, f"columns overflow => positive h-scroll extent, got {max_x_boot}"

        # ── (B) live-drag widen column 2 by +100px ──────────────────
        drag_grabber(tf, 2, 100)
        w2 = tf.query(col_path("width.2"))
        # The drag maps the cursor 1:1; allow ±2px for grabber-centre rounding.
        assert abs(w2 - (COL_W + 100)) <= 2, f"drag widened col 2 to ~230, got {w2}"
        assert_eq(tf.query(resize_path(2, "dragging")), False, "drag cleared after release")
        assert_eq(tf.query(col_path("width.0")), COL_W, "other columns unchanged by the drag")
        assert_eq(tf.query(col_path("width.7")), COL_W, "far column unchanged")
        assert_eq(tf.query(col_path("total")), NCOLS * COL_W + (w2 - COL_W),
                  "total grew by the drag delta")

        rects2 = snap_rects(tf)
        assert_eq(cell_w(rects2, "ghs#0_2"), w2, "rendered col-2 cell tracks the drag")
        assert_eq(cell_w(rects2, "ghs#0_3"), COL_W, "col-3 keeps its own width")
        assert_eq(cell_x(rects2, "ghs#0_3"), x3_boot + (w2 - COL_W),
                  "col-3 shifted right by the drag delta")
        max_x_wide = hscroll_max_x(tf)
        assert_eq(max_x_wide, max_x_boot + (w2 - COL_W),
                  "widening a column grew the h-scroll extent (resize ∘ h-scroll compose)")

        # ── (C) a second drag composes (widen column 0 by +60) ──────
        drag_grabber(tf, 0, 60)
        w0 = tf.query(col_path("width.0"))
        assert abs(w0 - (COL_W + 60)) <= 2, f"drag widened col 0 to ~190, got {w0}"
        assert_eq(tf.query(col_path("width.2")), w2, "col-2 still at its R(B) width")
        assert_eq(tf.query(col_path("total")), NCOLS * COL_W + (w2 - COL_W) + (w0 - COL_W),
                  "both resizes accumulate in the total")

        # ── (D) drag past the floor clamps to min_width ─────────────
        # Drag column 1's border far left (well past 0) — set_width floors it.
        drag_grabber(tf, 1, -400)
        assert_eq(tf.query(col_path("width.1")), min_w, "drag past the floor clamps to min_width")
        # The clamp is anchored on the press width, so a small right drag
        # un-clamps cleanly rather than sticking at the floor.
        drag_grabber(tf, 1, 200)
        assert tf.query(col_path("width.1")) > min_w, "border drags back out from the floor"

        # ── (E) zero-distance drag (click on the grabber) is a no-op ─
        before = tf.query(col_path("width.0"))
        drag_grabber(tf, 0, 0)
        assert_eq(tf.query(col_path("width.0")), before,
                  "a zero-distance grab (click) leaves the width unchanged")

        # ── (F) resize external introspection round-trip + rejections ─
        assert_eq(tf.query(resize_path(0, "col")), 0, "col 0 handle reports col 0")
        assert_eq(tf.query(resize_path(7, "col")), 7, "col 7 handle reports col 7")
        for bad in ("col", "dragging"):
            raised = False
            try:
                tf.intervene(resize_path(2, bad), 1)
            except RpcError:
                raised = True
            assert raised, f"resize external {bad!r} is read-only (intervene rejected)"
        raised = False
        try:
            tf.query(resize_path(2, "no_such_slot"))
        except RpcError:
            raised = True
        assert raised, "unknown resize introspect path is rejected"

    # ── PHASE 2 — native XTEST mouse drag + ffmpeg live pixels ──────
    native_live_pixel_guard()


# ===========================================================================
# Phase 2 — native pointer (XTest) + ffmpeg before/after
# ===========================================================================

# Minimal XTest driver: XTest.h dev headers are frequently absent, so declare
# the three prototypes we use directly and link the runtime shared objects.
XTEST_C = r"""
#include <X11/Xlib.h>
extern int XTestFakeMotionEvent(Display*, int, int, int, unsigned long);
extern int XTestFakeButtonEvent(Display*, unsigned int, int, unsigned long);
#include <stdlib.h>
#include <unistd.h>
/* argv: x0 y0 x1 y1 steps  — press at (x0,y0), march to (x1,y1), release. */
int main(int argc, char** argv) {
    if (argc < 6) return 2;
    int x0 = atoi(argv[1]), y0 = atoi(argv[2]);
    int x1 = atoi(argv[3]), y1 = atoi(argv[4]);
    int steps = atoi(argv[5]);
    Display* d = XOpenDisplay(NULL);
    if (!d) return 3;
    XTestFakeMotionEvent(d, -1, x0, y0, 0); XFlush(d); usleep(40000);
    XTestFakeButtonEvent(d, 1, 1, 0); XFlush(d); usleep(40000);
    for (int i = 1; i <= steps; i++) {
        int x = x0 + (x1 - x0) * i / steps;
        int y = y0 + (y1 - y0) * i / steps;
        XTestFakeMotionEvent(d, -1, x, y, 0); XFlush(d); usleep(20000);
    }
    XTestFakeButtonEvent(d, 1, 0, 0); XFlush(d); usleep(40000);
    XCloseDisplay(d);
    return 0;
}
"""


def native_live_pixel_guard() -> None:
    cc = shutil.which("cc")
    ffmpeg = shutil.which("ffmpeg")
    xwininfo = shutil.which("xwininfo")
    display = os.environ.get("DISPLAY")
    if not (cc and ffmpeg and xwininfo and display):
        print("  PHASE 2 SKIP: native XTEST guard needs cc + ffmpeg + xwininfo + DISPLAY")
        return
    try:
        from PIL import Image  # noqa: F401
    except ImportError:
        print("  PHASE 2 SKIP: Pillow unavailable")
        return

    tmp = Path(tempfile.mkdtemp(prefix="pinion-r786-"))
    helper = tmp / "xtest_drag"
    csrc = tmp / "xtest_drag.c"
    try:
        csrc.write_text(XTEST_C)
        cc_res = subprocess.run(
            [cc, str(csrc), "-o", str(helper), "-lX11", "-l:libXtst.so.6"],
            capture_output=True, text=True, timeout=60,
        )
        if cc_res.returncode != 0:
            print(f"  PHASE 2 SKIP: XTest helper did not compile:\n{cc_res.stderr.strip()}")
            return

        with RpcSubprocess(EXAMPLE, boot_grace=1.8, visible_window=True) as tf:
            win = _find_window_geom(xwininfo)
            if win is None:
                print("  PHASE 2 SKIP: could not locate the live window geometry")
                return
            wx, wy, ww, wh = win
            # Map the col-2 grabber centre (viewport coords) onto the screen,
            # honouring any window<->viewport scale (DPI / WM resize).
            rects = snap_rects(tf)
            if grabber_tag(2) not in rects:
                print("  PHASE 2 SKIP: col-2 grabber not in the live paint scene")
                return
            gx, gy, gw, gh = rects[grabber_tag(2)]
            sx = ww / WIN[0]
            sy = wh / WIN[1]
            cx0 = int(wx + (gx + gw / 2) * sx)
            cy0 = int(wy + (gy + gh / 2) * sy)
            drag_px = 120
            cx1 = cx0 + drag_px

            w2_before = tf.query(col_path("width.2"))
            before_png = tmp / "before.png"
            after_png = tmp / "after.png"
            _capture(ffmpeg, display, ww, wh, wx, wy, before_png)

            run = subprocess.run(
                [str(helper), str(cx0), str(cy0), str(cx1), str(cy0), "12"],
                capture_output=True, text=True, timeout=20,
            )
            if run.returncode != 0:
                print(f"  PHASE 2 SKIP: XTest drag exited {run.returncode} (no live X pointer?)")
                return
            time.sleep(1.2)  # >=1s settle so the after-frame is past mid-repaint
            w2_after = tf.query(col_path("width.2"))
            _capture(ffmpeg, display, ww, wh, wx, wy, after_png)

            # Decisive witness: the NATIVE pointer drove the full winit -> router
            # -> capture -> pointer_move arc, so the width model grew (the RPC
            # path was never used in this phase).
            grew = w2_after - w2_before
            assert grew > drag_px // 3, (
                f"native drag must widen col 2 through the winit path: "
                f"{w2_before} -> {w2_after} (Δ{grew}, drag {drag_px}px)"
            )
            # And the on-screen pixels actually changed (paint-readback != screen).
            changed = _frac_pixels_changed(before_png, after_png)
            assert changed > 0.01, (
                f"the live window pixels must change after the drag, got {changed:.4f}"
            )
            print(f"  PHASE 2 OK: native drag widened col 2 {w2_before}->{w2_after}px; "
                  f"{changed*100:.1f}% of window pixels changed")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _find_window_geom(xwininfo: str):
    """Return (x, y, w, h) of the pinion window's content rect, or None."""
    try:
        out = subprocess.run(
            [xwininfo, "-root", "-tree"], capture_output=True, text=True, timeout=10
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    # Match a mapped client of the expected viewport width (520) — the example
    # opens a fixed 520x440 window. Lines read like:  0xID "title": ("class")  WxH+X+Y  +X+Y
    import re
    for line in out.splitlines():
        m = re.search(r"(\d+)x(\d+)\+(-?\d+)\+(-?\d+)\s+\+(-?\d+)\+(-?\d+)", line)
        if not m:
            continue
        w, h = int(m.group(1)), int(m.group(2))
        # Accept a window whose width is the viewport (scale 1) or a clean DPI
        # multiple of it; the absolute origin is the last "+X+Y" pair.
        if abs(w - WIN[0]) <= 8 or abs(w - 2 * WIN[0]) <= 8:
            ax, ay = int(m.group(5)), int(m.group(6))
            return (ax, ay, w, h)
    return None


def _capture(ffmpeg: str, display: str, w: int, h: int, x: int, y: int, out: Path) -> None:
    subprocess.run(
        [ffmpeg, "-y", "-f", "x11grab", "-video_size", f"{w}x{h}",
         "-i", f"{display}+{x},{y}", "-frames:v", "1", "-pix_fmt", "rgba", str(out)],
        capture_output=True, timeout=15,
    )


def _frac_pixels_changed(a: Path, b: Path, tol: int = 16) -> float:
    """Fraction of pixels differing by more than `tol` on any channel."""
    from PIL import Image
    if not (a.exists() and b.exists()):
        return 0.0
    ia = Image.open(a).convert("RGB")
    ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        return 1.0
    pa, pb = ia.load(), ib.load()
    w, h = ia.size
    diff = 0
    total = 0
    # Sample on a stride for speed (every 3rd pixel) — a coarse change metric.
    for yy in range(0, h, 3):
        for xx in range(0, w, 3):
            total += 1
            ca, cb = pa[xx, yy], pb[xx, yy]
            if any(abs(ca[i] - cb[i]) > tol for i in range(3)):
                diff += 1
    return diff / max(total, 1)


if __name__ == "__main__":
    sys.exit(run_demo("R786 §5.27 — live-drag column resize", body))
