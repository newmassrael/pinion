#!/usr/bin/env python3
"""R1506 §5.27 §5.36 §2#7 — the declared alignment moves REAL screen pixels.

R1505 proved the chain `surface -> label node -> glyphs` at every link but one
kind of witness: its pixel guard renders headlessly, into an offscreen wgpu
target. That is the deterministic half. It says nothing about the live window
path — presentation, the compositor, the actual screen — which is where the
x11grab demos (r706 / r707 / r786 / r794 / r806) have been the precedent since
R706, and R1505 recorded its absence as the round's honest gap.

This closes it, and deliberately does NOT try to locate the window.

    Locating a window on a 1920x1080 screen means matching a colour and
    walking, which is what r706 does because it hunts one distinctive blue
    ring. A header label is ordinary dark text on ordinary light chrome and
    has no such anchor. So the capture localizes ITSELF: take one frame per
    alignment without moving the window, and the pixels that DIFFER between
    the `Start` frame and the `End` frame are, by construction, exactly the
    region the labels swept. Everything else on screen is identical.

Inside that region the ink centroid must slide rightward across Start ->
Center -> End. Nothing else in the app moves, so nothing else can produce that
signal.

Honest limit: this is an AGGREGATE over the whole strip, not a per-section
assertion. A change that slid four labels right and one left could in principle
average out. The per-section geometry is asserted deterministically in
`r1505_alignment_reaches_glyphs.py`, and the per-alignment ink extent in the
headless guard; what this adds, and all it claims to add, is that the
declaration survives to a real screen.

Requires a live X display + ffmpeg + Pillow. When any is unavailable the demo
SKIPs rather than failing — the absence of a capturable display is not
evidence of a defect.

Run:
    cargo build -p hello-column-reorder --release
    python3 tools/demos/r1506_alignment_live_pixels.py
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

SHOT = "/tmp/r1506_alignment_live_{}.png"
ALIGNMENTS = ("Start", "Center", "End")
# A pixel counts as ink when it is clearly darker than the header chrome. The
# labels are `OnSurface` over `SurfaceContainerHigh`, so the gap is wide; this
# only has to separate glyph strokes from the panel, not measure coverage.
INK_MAX = 140
# Below this many differing pixels the capture did not catch the repaint (or
# caught a stale frame), which is a harness problem, not a verdict.
MIN_DIFF_PIXELS = 200


def _skip(reason: str) -> int:
    print(f"SKIP: {reason}")
    return 0


def _screen_size() -> str | None:
    """The X screen's real dimensions.

    The sibling x11grab demos hardcode `1920x1080`, which on a larger screen
    silently captures only its top-left corner — the window under test may not
    be in it. Asking costs one subprocess and removes a way for this demo to
    report a confident answer about pixels it never looked at.
    """
    display = os.environ.get("DISPLAY", ":0")
    try:
        out = subprocess.run(
            ["xdpyinfo", "-display", display],
            capture_output=True, text=True, timeout=10,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    for line in out.splitlines():
        if "dimensions:" in line:
            return line.split()[1]
    return None


def _grab(path: str, size: str | None) -> bool:
    cmd = ["ffmpeg", "-y", "-f", "x11grab"]
    if size:
        cmd += ["-video_size", size]
    cmd += ["-i", os.environ.get("DISPLAY", ":0") + ".0", "-frames:v", "1", path]
    r = subprocess.run(cmd, capture_output=True, timeout=15)
    return r.returncode == 0 and Path(path).exists()


def _wait_rule(tf, align: str) -> bool:
    deadline = time.monotonic() + 10.0
    while time.monotonic() < deadline:
        if tf.query("/external/default_alignment") == align:
            return True
        time.sleep(0.1)
    return False


def _grab_settled(path: str, size: str | None) -> bool:
    """Grab until two consecutive frames are IDENTICAL, then keep the last.

    Settling on stability rather than on "the frame changed" is the whole
    point. The first draft waited for the frame to differ from the previous
    alignment's, which conflates two different facts — "the repaint has not
    landed yet" and "the repaint landed and moved nothing" — and reports the
    second as a SKIP. A counterfactual that removed the declaration from the
    production label made this demo exit 0. A harness more permissive than
    reality passes itself (R1496); this one cannot tell the difference between
    a slow compositor and a broken feature, so it must not try to.
    """
    scratch = ["/tmp/r1506_settle_a.png", "/tmp/r1506_settle_b.png"]
    deadline = time.monotonic() + 10.0
    prev = None
    i = 0
    while time.monotonic() < deadline:
        cur = scratch[i % 2]
        if not _grab(cur, size):
            return False
        if prev is not None and _diff_count(prev, cur) == 0:
            shutil.copyfile(cur, path)
            return True
        prev = cur
        i += 1
        time.sleep(0.2)
    return False


def _diff_count(a_path: str, b_path: str) -> int:
    from PIL import Image, ImageChops

    a = Image.open(a_path).convert("RGB")
    b = Image.open(b_path).convert("RGB")
    mask = ImageChops.difference(a, b).convert("L").point(
        lambda v: 1 if v > 40 else 0)
    return sum(mask.getdata())


def _diff_bbox(a_path: str, b_path: str):
    """Bounding box of pixels differing between two frames, and the count."""
    from PIL import Image, ImageChops

    a = Image.open(a_path).convert("RGB")
    b = Image.open(b_path).convert("RGB")
    diff = ImageChops.difference(a, b).convert("L")
    # Point-threshold before bbox so compositor noise does not widen the
    # region to the whole screen.
    mask = diff.point(lambda v: 255 if v > 40 else 0)
    count = sum(mask.point(lambda v: 1 if v else 0).getdata())
    return (mask.getbbox() if count >= MIN_DIFF_PIXELS else None), count


def _ink_centroid(path: str, box) -> float | None:
    """Mean x of the dark pixels inside `box`, in box-local coordinates."""
    from PIL import Image

    img = Image.open(path).convert("L").crop(box)
    w, h = img.size
    px = img.load()
    total = 0
    weighted = 0
    for y in range(h):
        for x in range(w):
            if px[x, y] <= INK_MAX:
                total += 1
                weighted += x
    if total == 0:
        return None
    return weighted / total


def body() -> int:
    with isolated_storage_dir("r1506-alignment-live"):
        with RpcSubprocess("hello-column-reorder", visible_window=True) as tf:
            size = _screen_size()
            print(f"[r1506] capturing {size or 'full screen'}")

            # ── PREMISE: is this app's window actually on the captured
            # screen? Established with a change that MUST move pixels
            # whatever the alignment does — hiding a whole column. Without
            # this the demo cannot tell "the window is not capturable" (a
            # legitimate SKIP on a headless box) from "the feature is
            # broken" (a FAIL), and would report the second as the first.
            base = "/tmp/r1506_alignment_live_base.png"
            probe = "/tmp/r1506_alignment_live_probe.png"
            if not _grab_settled(base, size):
                return _skip("the screen never settled (no capturable display)")
            tf.invoke("/external/set_section_hidden", "1:true")
            if not _grab_settled(probe, size):
                return _skip("the screen never settled after the liveness probe")
            _, probe_delta = _diff_bbox(base, probe)
            tf.invoke("/external/set_section_hidden", "1:false")
            if probe_delta < MIN_DIFF_PIXELS:
                return _skip(
                    f"hiding a column moved only {probe_delta} screen pixels — "
                    "this window is not visible in the capture (obscured, "
                    "unmapped, or presenting nothing), so nothing here can be "
                    "concluded about alignment"
                )
            print(f"[r1506] liveness: hiding a column moved {probe_delta} px")

            # ── MEASUREMENT: from here a screen that does not change is a
            # VERDICT, not a timing problem.
            shots = {}
            for align in ALIGNMENTS:
                path = SHOT.format(align.lower())
                tf.intervene("/external/default_alignment", align)
                if not _wait_rule(tf, align):
                    print(f"FAIL: the surface never reported {align}")
                    return 1
                if not _grab_settled(path, size):
                    return _skip(f"the screen never settled for {align}")
                shots[align] = path

            box, count = _diff_bbox(shots["Start"], shots["End"])
            assert box is not None, (
                f"Start and End differ in only {count} screen pixels. The "
                f"window IS capturable (the liveness probe moved "
                f"{probe_delta}), so the declared alignment did not reach the "
                f"presented window."
            )
            print(f"[r1506] label sweep region {box}, {count} differing px")

            centroids = {}
            for align, path in shots.items():
                c = _ink_centroid(path, box)
                assert c is not None, (
                    f"no ink in the sweep region for {align} — the region was "
                    f"derived from a real difference, so it must contain ink"
                )
                centroids[align] = c
                print(f"[r1506]   {align:<6} ink centroid x = {c:.1f}")

            s, m, e = (centroids[a] for a in ALIGNMENTS)
            assert s < m < e, (
                f"on a real screen the declared alignment must slide the ink "
                f"rightward: Start={s:.1f} Center={m:.1f} End={e:.1f}. Equal "
                f"or out-of-order centroids mean the declaration reached the "
                f"scene but not the presented window."
            )
            # Meaningfully apart, not merely ordered by a sub-pixel of AA.
            assert (m - s) >= 4 and (e - m) >= 4, (
                f"the steps must be real movement, not AA jitter: "
                f"Start->Center {m - s:.1f}px, Center->End {e - m:.1f}px"
            )
            print(
                f"PASS: ink centroid slid {s:.1f} -> {m:.1f} -> {e:.1f} "
                f"({e - s:.1f}px total) on the live window."
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
