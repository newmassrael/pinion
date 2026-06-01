#!/usr/bin/env python3
"""R740 §5.16 — raster image paint (Scene::Image no-op cleared) E2E.

Drives the new `hello-image` binding via JSON-RPC. The first consumer of
the asset pipeline: a bundled 16x16 four-quadrant PNG (red / green / blue /
white, dark border) decoded once (pinion-asset) and painted four times,
once per Fit policy (Fill / Contain / Cover / Tile), into identical
non-square 150x80 cells.

Atomic verification scope (>=30 assertions):

  (A) introspection (scene-as-data, §2 #7) — the four image nodes are
      present with the right tag, the same decoded source path, the right
      Fit policy, and a 150x80 rect. An AI agent verifies "the right icon
      is shown under the right fit" without a single pixel.
  (B) the toggle is live — scene/click flips the highlight value (the
      cell-outline accent); the binding works through the RPC click path.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The decisive proof that the image really decoded and rendered:
      * Fill cell — its four quadrant interiors sample red / green / blue /
        white. No solid fill, gradient, or single-colour primitive can
        produce four different quadrant colours; only the decoded bitmap
        can. This is the killer witness.
      * Cover vs Fill — Fill frames the cell on all four sides (the source
        border stretches to every edge), while Cover crops the top/bottom
        border away (only left/right remain). So the Fill cell's
        top-centre is the dark border while the Cover cell's top-centre is
        a bright quadrant colour — the two fits are distinct.
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
)

EXAMPLE = "hello-image"
VIEWPORT = (360, 300)
PAUSE = 0.1
FITS = {
    "img_fill": "Fill",
    "img_contain": "Contain",
    "img_cover": "Cover",
    "img_tile": "Tile",
}


def near(a: float, b: float, tol: int = 40) -> bool:
    return abs(int(a) - int(b)) <= tol


def is_color(px, r, g, b, tol: int = 60) -> bool:
    return near(px[0], r, tol) and near(px[1], g, tol) and near(px[2], b, tol)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)

        # ── (A) the four image nodes — tag / source / fit / rect ────
        rects = abs_rects_of(snap)
        seen_source = None
        for tag, expected_fit in FITS.items():
            node = find_by_tag(snap, tag)
            assert node is not None, f"{tag} image node present at boot"
            assert node.get("type") == "Image", f"{tag} is an image node"
            src = node.get("source") or ""
            assert src.endswith("pinion-hello-image-quadrants.png"), \
                f"{tag} source is the bundled icon path, got {src!r}"
            # All four share the SAME decoded source (one decode, four draws).
            seen_source = seen_source or src
            assert_eq(src, seen_source, f"{tag} shares the one decoded source")
            style = node.get("style") or {}
            assert_eq(style.get("fit"), expected_fit, f"{tag} carries fit={expected_fit}")
            assert style.get("tint") is None, f"{tag} has no tint (deferred axis)"
            assert tag in rects, f"{tag} has an absolute rect"
            _, _, w, h = rects[tag]
            assert near(w, 150, 4) and near(h, 80, 4), \
                f"{tag} rect is the 150x80 cell, got {w}x{h}"

        # ── (B) the toggle is live (RPC click flips the highlight) ──
        before = tf.query("/external/value")
        assert before is False, "highlight starts muted (value false)"
        tf.click(path="main_toggle"); time.sleep(PAUSE)
        assert tf.query("/external/value") is True, "click flips highlight to accent"
        tf.click(path="main_toggle"); time.sleep(PAUSE)
        assert tf.query("/external/value") is False, "second click flips back to muted"
        assert_eq(tf.query("/external/state"), "Hover", "toggle settles in Hover after click")

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"

    # Fill cell: four quadrant interiors = red / green / blue / white.
    fx, fy, fw, fh = rects["img_fill"]
    def cell_pt(rx, ry, frac_x, frac_y):
        return (rx + int(frac_x * fw), ry + int(frac_y * fh))
    tl, tr, bl, br = sample_png_points(img, [
        cell_pt(fx, fy, 0.28, 0.30),  # top-left  → red
        cell_pt(fx, fy, 0.72, 0.30),  # top-right → green
        cell_pt(fx, fy, 0.28, 0.72),  # bot-left  → blue
        cell_pt(fx, fy, 0.72, 0.72),  # bot-right → white
    ])
    assert is_color(tl, 220, 40, 40), f"Fill top-left quadrant is red, got {tl}"
    assert is_color(tr, 40, 200, 40), f"Fill top-right quadrant is green, got {tr}"
    assert is_color(bl, 40, 80, 220), f"Fill bottom-left quadrant is blue, got {bl}"
    assert is_color(br, 240, 240, 240), f"Fill bottom-right quadrant is white, got {br}"
    # Four distinct colours from one primitive = a real decoded bitmap.
    assert len({tl, tr, bl, br}) == 4, "Fill cell shows four distinct quadrant colours"

    # Cover vs Fill: Fill borders all four sides, Cover crops top/bottom.
    # Sample each cell's top-centre: Fill = dark border, Cover = bright.
    cx, cy, cw, ch = rects["img_cover"]
    fill_top = sample_png_points(img, [(fx + fw // 2, fy + 3)])[0]
    cover_top = sample_png_points(img, [(cx + cw // 2, cy + 3)])[0]
    assert is_color(fill_top, 30, 30, 30, tol=50), \
        f"Fill top edge is the stretched dark border, got {fill_top}"
    assert not is_color(cover_top, 30, 30, 30, tol=50), \
        f"Cover top edge crops the border (bright quadrant), got {cover_top}"
    assert fill_top != cover_top, "Fill and Cover are visibly distinct fits"

    # Contain: the square image is centred, so the cell's far-left strip is
    # the page letterbox margin (not part of the image) — distinct from the
    # Fill cell which has no margin.
    ox, oy, ow, oh = rects["img_contain"]
    contain_left = sample_png_points(img, [(ox + 3, oy + oh // 2)])[0]
    fill_left = sample_png_points(img, [(fx + 3, fy + fh // 2)])[0]
    assert contain_left != fill_left, \
        f"Contain letterboxes (margin) where Fill does not (margin={contain_left} fill={fill_left})"

    # Tile: the 16x16 icon repeats, so the cell interior contains both bright
    # quadrant colour AND the dark border many times — at least one dark
    # border pixel appears well inside the cell (a single placement would
    # not put a border there).
    tlx, tly, tlw, tlh = rects["img_tile"]
    tile_mid = sample_png_points(img, [
        (tlx + int(0.5 * tlw), tly + int(0.5 * tlh)),
        (tlx + int(0.33 * tlw), tly + int(0.5 * tlh)),
        (tlx + int(0.66 * tlw), tly + int(0.5 * tlh)),
    ])
    assert any(is_color(p, 30, 30, 30, tol=50) for p in tile_mid), \
        f"Tile repeats the bordered icon (a dark border appears mid-cell), got {tile_mid}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r740-")) / "image.png"
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
    sys.exit(run_demo("R740 §5.16 — raster image paint (Scene::Image)", body))
