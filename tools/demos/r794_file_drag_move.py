#!/usr/bin/env python3
"""R794 §5.51 — drag-to-move in the own-rendered file manager, over RPC.

The R742 pointer drag-drop substrate gains a new *consumer kind*: the file
manager's `DirectoryExternal` is now a drag source + resolver, so dragging a
row onto a folder **moves** the entry into it (cross-directory rename), and
dragging onto the `../` breadcrumb moves it up to the parent. This is the core
OS-file-manager interaction (Finder/Explorer drag-to-move) the own-rendered
file UI was missing — now AI-drivable through `scene/drag` and verifiable as
scene-as-data (§2 #2 / #7), no native file manager required.

The move is committed by `Directory::rename(from, to)` (the R791 write
surface already did cross-dir moves); R794 adds the pointer affordance + the
introspectable in-flight drag state (`dragging` / `drop_target`).

  (A) boot — /proj lists [src, assets, Cargo.toml, README.md]; idle drag
      state (`dragging` false, `drop_target` Null).
  (B) inert drops — dragging a file onto another *file* row, or onto itself,
      moves nothing (only a folder / the breadcrumb is a valid target).
  (C) move a file into a folder — drag README.md onto assets/: it leaves
      /proj and appears inside /proj/assets.
  (D) move a file into another folder — drag Cargo.toml onto src/.
  (E) move a folder into a folder (subtree carried) — drag assets/ onto src/:
      /proj/assets and its contents re-home under /proj/src/assets.
  (F) move up to the parent — inside src/, drag a file onto the `../`
      breadcrumb: it lands back in /proj.
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
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-file-manager"
VIEWPORT = (460, 520)
WIN = (480, 540)  # the example's fixed window size (WIN_W x WIN_H)
ACCENT = (0x19, 0x76, 0xD2)  # ColorRole::Accent (light) — the drop-target outline

DIR = "fb_dir"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def names(tf) -> list[str]:
    """Current listing leaf names (dirs lose the wire '/' suffix)."""
    wire = tf.query(dpath("entries"))
    if not wire:
        return []
    return [n[:-1] if n.endswith("/") else n for n in wire.split("\n")]


def count(tf) -> int:
    return tf.query(dpath("count"))


def row_index(tf, name: str) -> int:
    for i in range(count(tf)):
        if tf.query(dpath(f"name.{i}")) == name:
            return i
    raise AssertionError(f"{name} not in listing {names(tf)}")


def row_tag(tf, name: str) -> str:
    return f"{DIR}#{row_index(tf, name)}"


def settle(tf) -> None:
    """Refresh the painted layout so the next `scene/drag` resolves row
    rects against the *current* listing. A paint snapshot re-runs the
    producer and re-stores the frame inside its own dispatch (R705), so
    no wall-clock wait is needed (R883 zero-flake)."""
    tf.snapshot(source="paint", viewport=VIEWPORT)


def navigate(tf, name: str) -> str:
    cwd = tf.invoke(dpath("navigate"), name)
    settle(tf)
    return cwd


def up(tf) -> str:
    cwd = tf.invoke(dpath("up"), None)
    settle(tf)
    return cwd


def drag_row_onto(tf, src_name: str, dst_tag: str) -> None:
    tf.drag(from_path=row_tag(tf, src_name), to_path=dst_tag, steps=10)
    settle(tf)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5, visible_window=True) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, DIR) is not None, "the file list paints"
        assert_eq(count(tf), 4, "/proj boots with four entries")
        assert "src" in names(tf) and "assets" in names(tf), "seeded folders listed"
        assert "Cargo.toml" in names(tf) and "README.md" in names(tf), "seeded files listed"
        assert_eq(tf.query(dpath("dragging")), False, "no drag armed at boot")
        assert_eq(tf.query(dpath("drop_target")), None, "no drop target at boot")

        # ── (B) inert drops (only a folder / the breadcrumb is a target) ─
        # File onto another *file* row → nothing moves.
        drag_row_onto(tf, "Cargo.toml", row_tag(tf, "README.md"))
        assert_eq(count(tf), 4, "dropping a file onto a file moved nothing")
        assert "Cargo.toml" in names(tf), "Cargo.toml stayed put"
        assert "README.md" in names(tf), "README.md stayed put"
        # File onto itself → nothing moves.
        cargo_tag = row_tag(tf, "Cargo.toml")
        tf.drag(from_path=cargo_tag, to_path=cargo_tag, steps=6)
        settle(tf)
        assert_eq(count(tf), 4, "dropping a file onto itself moved nothing")
        assert_eq(tf.query(dpath("dragging")), False, "drag disarmed after an inert drop")
        assert_eq(tf.query(dpath("drop_target")), None, "drop target cleared after an inert drop")

        # ── (C) move a file into a folder ───────────────────────────
        drag_row_onto(tf, "README.md", row_tag(tf, "assets"))
        assert "README.md" not in names(tf), "README.md left /proj"
        assert_eq(count(tf), 3, "/proj shrank to three entries")
        navigate(tf, "assets")
        assert "README.md" in names(tf), "README.md is now inside /proj/assets"
        assert "logo.png" in names(tf), "the folder kept its original file"
        up(tf)
        assert_eq(tf.query(dpath("cwd")), "/proj", "back at /proj")

        # ── (D) move another file into a folder ─────────────────────
        drag_row_onto(tf, "Cargo.toml", row_tag(tf, "src"))
        assert "Cargo.toml" not in names(tf), "Cargo.toml left /proj"
        assert_eq(count(tf), 2, "/proj is now [src, assets]")
        navigate(tf, "src")
        assert "Cargo.toml" in names(tf), "Cargo.toml is now inside /proj/src"
        up(tf)

        # ── (E) move a folder into a folder (subtree carried) ───────
        drag_row_onto(tf, "assets", row_tag(tf, "src"))
        assert "assets" not in names(tf), "assets/ left /proj"
        assert_eq(count(tf), 1, "/proj is now just [src]")
        navigate(tf, "src")
        assert "assets" in names(tf), "assets/ re-homed under /proj/src"
        navigate(tf, "assets")
        assert "logo.png" in names(tf), "the moved folder carried its file"
        assert "README.md" in names(tf), "and the file moved into it earlier"
        up(tf)  # back to /proj/src
        assert_eq(tf.query(dpath("cwd")), "/proj/src", "back in /proj/src")

        # ── (F) move up to the parent via the `../` breadcrumb ──────
        # Cargo.toml lives in /proj/src now; drag it onto `../` → /proj.
        assert "Cargo.toml" in names(tf), "Cargo.toml is in /proj/src"
        drag_row_onto(tf, "Cargo.toml", f"{DIR}#up")
        assert "Cargo.toml" not in names(tf), "Cargo.toml left /proj/src"
        up(tf)
        assert_eq(tf.query(dpath("cwd")), "/proj", "back at /proj")
        assert "Cargo.toml" in names(tf), "Cargo.toml landed in /proj (the parent)"

        # ── idle drag introspection holds ──────────────────────────
        assert_eq(tf.query(dpath("dragging")), False, "no drag armed when idle")
        assert_eq(tf.query(dpath("drop_target")), None, "no drop target when idle")

    # ── PHASE 2 — native XTEST mouse drag + ffmpeg live pixels ──────
    native_live_pixel_guard()


# ===========================================================================
# Phase 2 — native pointer (XTest) + ffmpeg before/during/after
# ===========================================================================
#
# `scene/drag` injects at the InputRouter, BYPASSING winit. This phase drives a
# REAL X pointer (XTest) over the live window to prove the full native path
# (winit MouseInput / CursorMoved -> ShellCore -> InputRouter -> begin_drag /
# drag_to / drag_release). Two witnesses: (1) the file moves into the folder (the
# RPC path was never touched in this phase), and (2) the drop-target Accent
# outline actually appears on-screen *mid-drag* (the new R794 paint), captured
# while the button is held over the folder ([[introspection-from-paint-not-screen]]:
# a paint-readback is not a screen capture). Skips cleanly when cc / ffmpeg /
# Xtst / a live display / Pillow is unavailable (a tracked env carry).

# Minimal XTest driver. argv: x0 y0 x1 y1 steps hold_ms — press at (x0,y0),
# march to (x1,y1), HOLD pressed for hold_ms (so a mid-drag frame can be
# captured), then release.
XTEST_C = r"""
#include <X11/Xlib.h>
extern int XTestFakeMotionEvent(Display*, int, int, int, unsigned long);
extern int XTestFakeButtonEvent(Display*, unsigned int, int, unsigned long);
#include <stdlib.h>
#include <unistd.h>
int main(int argc, char** argv) {
    if (argc < 7) return 2;
    int x0 = atoi(argv[1]), y0 = atoi(argv[2]);
    int x1 = atoi(argv[3]), y1 = atoi(argv[4]);
    int steps = atoi(argv[5]);
    int hold_ms = atoi(argv[6]);
    Display* d = XOpenDisplay(NULL);
    if (!d) return 3;
    XTestFakeMotionEvent(d, -1, x0, y0, 0); XFlush(d); usleep(40000);
    XTestFakeButtonEvent(d, 1, 1, 0); XFlush(d); usleep(40000);
    for (int i = 1; i <= steps; i++) {
        int x = x0 + (x1 - x0) * i / steps;
        int y = y0 + (y1 - y0) * i / steps;
        XTestFakeMotionEvent(d, -1, x, y, 0); XFlush(d); usleep(20000);
    }
    usleep((useconds_t) hold_ms * 1000);   /* hold over the drop target */
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

    tmp = Path(tempfile.mkdtemp(prefix="pinion-r794-"))
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
            # Map row rects onto the screen using the *window* layout (the live
            # paint is at WIN, not the RPC-only VIEWPORT).
            rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
            # Drag README.md (a file) onto assets/ (a folder). The listing is
            # dirs-first alpha (assets before src), so resolve indices live.
            dst_idx = row_index(tf, "assets")
            src_tag = f"{DIR}#{row_index(tf, 'README.md')}"
            dst_tag = f"{DIR}#{dst_idx}"
            if src_tag not in rects or dst_tag not in rects:
                print("  PHASE 2 SKIP: source / target rows not in the live paint scene")
                return
            sx = ww / WIN[0]
            sy = wh / WIN[1]
            fx, fy, fw, fh = rects[src_tag]
            dx, dy, dw, dh = rects[dst_tag]
            cx0 = int(wx + (fx + fw / 2) * sx)
            cy0 = int(wy + (fy + fh / 2) * sy)
            cx1 = int(wx + (dx + dw / 2) * sx)
            cy1 = int(wy + (dy + dh / 2) * sy)
            # The folder row's rect in *window-relative* coords (ffmpeg crops
            # the capture to the window origin, so png (0,0) == screen (wx,wy)).
            row_region = (
                int(dx * sx), int(dy * sy),
                max(1, int(dw * sx)), max(1, int(dh * sy)),
            )

            before_png = tmp / "before.png"
            during_png = tmp / "during.png"
            after_png = tmp / "after.png"
            _capture(ffmpeg, display, ww, wh, wx, wy, before_png)

            # Launch the drag in the background; it holds 1s over the folder.
            proc = subprocess.Popen(
                [str(helper), str(cx0), str(cy0), str(cx1), str(cy1), "12", "1000"],
            )
            # R883 zero-flake: gate on the LIVE drag introspection — the
            # drop_target slot flips to the folder row once the native
            # march arrives over it (mid-hold), replacing the fixed
            # press+march timing guess.
            wait_until(
                lambda: tf.query(dpath("drop_target")) == f"row:{dst_idx}",
                timeout=6.0, interval=0.04,
                desc="native drag hold reports assets as the drop target",
            )
            mid_drop = tf.query(dpath("drop_target"))  # introspect the live drag
            # Poll the SCREEN during the hold until the Accent outline is
            # presented (the stored-frame re-store is sync, the real
            # present is not).
            acc_before = _count_accent(before_png, row_region)
            _capture(ffmpeg, display, ww, wh, wx, wy, during_png)
            hold_deadline = time.monotonic() + 0.9
            while (_count_accent(during_png, row_region) <= acc_before + 30
                   and time.monotonic() < hold_deadline):
                _capture(ffmpeg, display, ww, wh, wx, wy, during_png)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                print("  PHASE 2 SKIP: XTest drag did not complete")
                return
            if proc.returncode != 0:
                print(f"  PHASE 2 SKIP: XTest drag exited {proc.returncode} (no live X pointer?)")
                return
            # Witness 1 — the NATIVE pointer drove winit -> router -> drag_release,
            # so README.md moved into assets/ (the RPC path was untouched here).
            # Gate on the observed listing (the helper exited, but the
            # X event delivery into winit is async).
            wait_until(
                lambda: "README.md" not in names(tf),
                desc="native drag must move README.md out of /proj via the winit path",
            )
            _capture(ffmpeg, display, ww, wh, wx, wy, after_png)
            assert_eq(tf.invoke(dpath("navigate"), "assets"), "/proj/assets",
                      "navigate into the drop folder")
            assert "README.md" in names(tf), "README.md landed inside /proj/assets"

            # Witness 2 — the drop-target Accent outline appeared on-screen during
            # the hold (the new R794 paint), and the live pixels actually changed.
            assert mid_drop == f"row:{dst_idx}", \
                f"the held drag reports assets (row {dst_idx}) as the drop target, got {mid_drop}"
            acc_during = _count_accent(during_png, row_region)
            assert acc_during > acc_before + 30, (
                f"the Accent drop outline must appear on the folder row during the drag: "
                f"accent px before={acc_before} during={acc_during}"
            )
            print(f"  PHASE 2 OK: native drag moved README.md into assets/; "
                  f"drop outline accent px {acc_before}->{acc_during}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _find_window_geom(xwininfo: str):
    """Return (x, y, w, h) of the pinion window's content rect, or None."""
    import re
    try:
        out = subprocess.run(
            [xwininfo, "-root", "-tree"], capture_output=True, text=True, timeout=10
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in out.splitlines():
        m = re.search(r"(\d+)x(\d+)\+(-?\d+)\+(-?\d+)\s+\+(-?\d+)\+(-?\d+)", line)
        if not m:
            continue
        w, h = int(m.group(1)), int(m.group(2))
        if abs(w - WIN[0]) <= 8 or abs(w - 2 * WIN[0]) <= 8:
            return (int(m.group(5)), int(m.group(6)), w, h)
    return None


def _capture(ffmpeg: str, display: str, w: int, h: int, x: int, y: int, out: Path) -> None:
    subprocess.run(
        [ffmpeg, "-y", "-f", "x11grab", "-video_size", f"{w}x{h}",
         "-i", f"{display}+{x},{y}", "-frames:v", "1", "-pix_fmt", "rgba", str(out)],
        capture_output=True, timeout=15,
    )


def _count_accent(png: Path, region: tuple[int, int, int, int], tol: int = 40) -> int:
    """Count pixels within `region` (absolute screen coords) close to ACCENT."""
    from PIL import Image
    if not png.exists():
        return 0
    img = Image.open(png).convert("RGB")
    rx, ry, rw, rh = region
    iw, ih = img.size
    px = img.load()
    n = 0
    for yy in range(max(0, ry), min(ih, ry + rh)):
        for xx in range(max(0, rx), min(iw, rx + rw)):
            r, g, b = px[xx, yy]
            if abs(r - ACCENT[0]) <= tol and abs(g - ACCENT[1]) <= tol and abs(b - ACCENT[2]) <= tol:
                n += 1
    return n


if __name__ == "__main__":
    run_demo("r794_file_drag_move", body)
