#!/usr/bin/env python3
"""R723 §5.50 inverse-surface ColorRole tier demo — RPC introspection + live-pixel.

R723 adds the Material 3 *inverse* colour tier — `inverseSurface`,
`inverseOnSurface`, `inversePrimary` — to `ColorRole`, mirroring the R590 error
tier (additive enum variants + Theme fields + ThemeLinear carrier for the
R57.X theme-fade). hello-theme gains an `inverse_swatch` that renders the paired
`inverseSurface` fill / `inverseOnSurface` text, so the new roles are
introspectable as data and verifiable by live-pixel.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=24 assertions). The inverse swatch's
    fill is the light `inverseSurface` tone (#322F35), its label the
    `inverseOnSurface` tone (#F5EFF7); both are distinct from `surface` and from
    each other (the inverse tier genuinely inverts). Toggling light->dark swaps
    the role toward the dark `inverseSurface` (#E6E1E5) — the role tracks the
    palette (fade-tolerant: lighter, no longer the light value).

  Phase 2 — PIXELS (PINION_SCREENSHOT). The swatch interior renders the dark
    `inverseSurface` tone on the light boot frame, and the panel background is
    the light `surface` — proving the new role reaches paint, not just data.

Run from the workspace root:
    cargo build -p hello-theme --release
    python3 tools/demos/r723_theme_inverse.py
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
    abs_rects_of,
    assert_eq,
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

VIEWPORT = (360, 300)

# Mirror the light-palette inverse `const`s in crates/pinion-core/src/theme.rs.
INVERSE_SURFACE_LIGHT = (0x32, 0x2F, 0x35)
INVERSE_ON_SURFACE_LIGHT = (0xF5, 0xEF, 0xF7)
SURFACE_LIGHT = (0xFF, 0xFF, 0xFF)


def rgb(c: dict) -> tuple[int, int, int]:
    return (c["r"], c["g"], c["b"])


def body() -> None:
    swatch_rect = None
    with RpcSubprocess("hello-theme") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        swatch = find_by_tag(snap, "inverse_swatch")
        assert swatch is not None, "inverse_swatch present"
        assert_eq(swatch["type"], "Container", "inverse_swatch is a container")

        # Fill = light inverseSurface.
        fill = swatch["style"]["fill"]
        assert_eq(fill["r"], INVERSE_SURFACE_LIGHT[0], "swatch fill r")
        assert_eq(fill["g"], INVERSE_SURFACE_LIGHT[1], "swatch fill g")
        assert_eq(fill["b"], INVERSE_SURFACE_LIGHT[2], "swatch fill b")

        # Label text = light inverseOnSurface.
        children = swatch["children"]
        assert len(children) >= 1, "swatch has a label child"
        label = children[0]
        assert_eq(label["type"], "Text", "swatch child is text")
        assert_eq(label["content"], "Inverse surface", "swatch label content")
        fg = label["style"]["fg_color"]
        assert_eq(fg["r"], INVERSE_ON_SURFACE_LIGHT[0], "label fg r")
        assert_eq(fg["g"], INVERSE_ON_SURFACE_LIGHT[1], "label fg g")
        assert_eq(fg["b"], INVERSE_ON_SURFACE_LIGHT[2], "label fg b")

        # The inverse tier genuinely inverts: fill is dark, text is light,
        # and both differ from the panel `surface` (white).
        assert rgb(fill) != SURFACE_LIGHT, "inverseSurface != surface"
        assert rgb(fg) != rgb(fill), "inverseOnSurface != inverseSurface"
        assert sum(rgb(fill)) < sum(rgb(fg)), "inverseSurface darker than its text"
        # Panel root fill is the light surface.
        root_fill = snap["style"]["fill"]
        assert_eq(rgb(root_fill), SURFACE_LIGHT, "panel surface is white")
        assert rgb(fill) != rgb(root_fill), "inverseSurface != panel surface"

        rects = abs_rects_of(snap)
        assert "inverse_swatch" in rects, "inverse_swatch has an absolute rect"
        swatch_rect = rects["inverse_swatch"]

        # ── Toggle light -> dark: the role tracks the palette ─────────
        light_fill = rgb(fill)
        d.click(path="theme_toggle")
        snap_dark = d.snapshot(source="paint", viewport=VIEWPORT)
        dark_fill = rgb(find_by_tag(snap_dark, "inverse_swatch")["style"]["fill"])
        # Fade-tolerant: dark inverseSurface (#E6E1E5) is far lighter than the
        # light tone (#322F35); even mid-fade the sum has risen and the value
        # has left the light tone.
        assert dark_fill != light_fill, "inverseSurface changed on theme swap"
        assert sum(dark_fill) > sum(light_fill), "dark inverseSurface is lighter"

    # ── Phase 2 — live-pixel (light boot frame) ──────────────────────
    assert swatch_rect is not None
    sx, sy, sw, sh = swatch_rect
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"

    centre = (sx + sw // 2, sy + sh // 2)
    bg = (sx + sw // 2, 8)  # top strip — panel surface, clear of the swatch
    swatch_px, bg_px = sample_png_points(img, [centre, bg])
    assert_pixel_eq(swatch_px, (*INVERSE_SURFACE_LIGHT, 255),
                    f"inverse swatch interior {centre}", tolerance=12)
    assert_pixel_eq(bg_px, (*SURFACE_LIGHT, 255), f"panel surface {bg}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r723-")) / "theme_inverse.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-theme"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-theme", "--quiet", "--release",
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
    sys.exit(run_demo("R723 inverse-surface ColorRole tier", body))
