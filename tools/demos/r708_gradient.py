#!/usr/bin/env python3
"""R708 §5.50 gradient-fill substrate demo — RPC introspection + live-pixel.

The first non-solid `BoxStyle` paint. `BoxStyle.gradient` carries an optional
`Gradient` (linear / radial, box-relative UV geometry, an unbounded stop ramp,
an `Extend` mode) that the Vello `paint_adapter` lowers to a `peniko::Gradient`.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=40 assertions). The hue strip's
    gradient is read back as data — geometry kind, UV endpoints, the seven
    sRGB-primary stops with their offsets/colours, and the extend mode — with
    no OCR (§2 #7). Toggling the demo swatch linear -> radial is observed as a
    GradientKind change in the snapshot, proving the Off/On bit reaches paint.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render). The
    rendered hue strip is sampled at its exact stop offsets; the colour AT a
    stop is the stop colour regardless of the interpolation colour space, so
    the sample must match the stop within an AA tolerance. A regression that
    dropped the gradient would paint the strip's solid `fill` fallback (opaque
    black) and these asserts would fail loudly. The headless screenshot uses
    the same `to_vello_cached` rasterizer the live window does (producer
    parity), so a pixel here is a faithful witness.

Run from the workspace root:
    cargo build -p hello-gradient --release
    python3 tools/demos/r708_gradient.py
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

VIEWPORT = (360, 320)

# Hue-strip stop ramp — mirrors the `const`s in
# `examples/hello-gradient/src/main.rs` (SSOT for both sides).
HUE_STOPS = [
    (0.0, (0xFF, 0x00, 0x00)),        # red
    (1.0 / 6.0, (0xFF, 0xFF, 0x00)),  # yellow
    (2.0 / 6.0, (0x00, 0xFF, 0x00)),  # green
    (3.0 / 6.0, (0x00, 0xFF, 0xFF)),  # cyan
    (4.0 / 6.0, (0x00, 0x00, 0xFF)),  # blue
    (5.0 / 6.0, (0xFF, 0x00, 0xFF)),  # magenta
    (1.0, (0xFF, 0x00, 0x00)),        # red
]


def _gradient_of(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in paint snapshot"
    style = node.get("style")
    assert isinstance(style, dict), f"{tag} carries a style object"
    gradient = style.get("gradient")
    assert isinstance(gradient, dict), f"{tag} carries a gradient (not null)"
    return gradient


def assert_color(actual: dict, expected: tuple[int, int, int], label: str) -> None:
    got = (actual["r"], actual["g"], actual["b"])
    assert got == expected, f"{label}: color {got} != expected {expected}"


def _approx(actual: float, expected: float, label: str, eps: float = 1e-4) -> None:
    assert abs(actual - expected) < eps, f"{label}: {actual} != {expected}"


def capture_screenshot() -> Path:
    """Render hello-gradient's initial (Off / Idle) frame to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r708-")) / "gradient.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-gradient"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-gradient", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


def body() -> None:
    # ── Phase 1 — structural introspection ───────────────────────────
    strip_rect = None
    with RpcSubprocess("hello-gradient") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        # Hue strip — a 7-stop horizontal linear gradient.
        grad = _gradient_of(snap, "hue_strip")
        geom = grad["geometry"]
        assert_eq(geom["kind"], "linear", "hue strip geometry.kind")
        _approx(geom["start"]["u"], 0.0, "hue strip start.u")
        _approx(geom["start"]["v"], 0.0, "hue strip start.v")
        _approx(geom["end"]["u"], 1.0, "hue strip end.u")
        _approx(geom["end"]["v"], 0.0, "hue strip end.v")
        assert_eq(grad["extend"], "Pad", "hue strip extend")

        stops = grad["stops"]
        assert_eq(len(stops), 7, "hue strip stop count")
        for i, (off, rgb) in enumerate(HUE_STOPS):
            _approx(stops[i]["offset"], off, f"hue stop {i} offset", eps=1e-3)
            assert_color(stops[i]["color"], rgb, f"hue stop {i} color")

        # Demo swatch — linear (vertical) while Off.
        sw = _gradient_of(snap, "demo_swatch")
        assert_eq(sw["geometry"]["kind"], "linear", "swatch (Off) kind")
        _approx(sw["geometry"]["start"]["v"], 0.0, "swatch (Off) start.v")
        _approx(sw["geometry"]["end"]["v"], 1.0, "swatch (Off) end.v")
        assert_eq(len(sw["stops"]), 2, "swatch (Off) stop count")

        # Capture the strip's absolute rect for the pixel phase (same
        # viewport the headless screenshot renders, so it matches).
        rects = abs_rects_of(snap)
        assert "hue_strip" in rects, "hue strip has an absolute rect"
        strip_rect = rects["hue_strip"]

        # State before the toggle.
        assert_eq(d.query("/external/state"), "Idle", "initial /external/state")
        assert_eq(d.query("/external/value"), False, "initial /external/value (Off)")

        # ── Toggle Off -> On: swatch must switch linear -> radial ─────
        d.click(path="main_toggle")
        assert_eq(d.query("/external/value"), True, "/external/value after click (On)")

        snap_on = d.snapshot(source="paint", viewport=VIEWPORT)
        sw_on = _gradient_of(snap_on, "demo_swatch")
        assert_eq(sw_on["geometry"]["kind"], "radial", "swatch (On) kind")
        _approx(sw_on["geometry"]["center"]["u"], 0.5, "swatch (On) center.u")
        _approx(sw_on["geometry"]["center"]["v"], 0.5, "swatch (On) center.v")
        _approx(sw_on["geometry"]["radius"], 0.5, "swatch (On) radius")
        assert_eq(len(sw_on["stops"]), 2, "swatch (On) stop count")

        # The hue strip is mode-independent: still linear after toggling.
        assert_eq(
            _gradient_of(snap_on, "hue_strip")["geometry"]["kind"],
            "linear",
            "hue strip stays linear after toggle",
        )

    # ── Phase 2 — live-pixel verification of the hue strip ───────────
    assert strip_rect is not None
    sx, sy, sw_px, sh_px = strip_rect
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, (
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    )

    row_y = sy + sh_px // 2
    # Sample the three interior primary stops (green 1/3, cyan 1/2,
    # blue 2/3). AT a stop the colour is the stop colour irrespective of
    # interpolation space; sample a clamped interior pixel column.
    interior = [HUE_STOPS[2], HUE_STOPS[3], HUE_STOPS[4]]
    points = []
    for off, _rgb in interior:
        px_x = sx + max(1, min(sw_px - 2, round(sw_px * off)))
        points.append((px_x, row_y))
    samples = sample_png_points(img, points)
    for (off, rgb), (x, y), pixel in zip(interior, points, samples):
        # Tolerance 24: a 1-px rounding at the sample column lands a hair
        # off the exact stop, and Area-AA adds a small band; the primary
        # hue is unmistakable well inside this.
        assert_pixel_eq(
            pixel, (rgb[0], rgb[1], rgb[2], 255),
            f"hue strip at offset {off:.3f} ({x}, {y})", tolerance=24,
        )


if __name__ == "__main__":
    sys.exit(run_demo("R708 gradient-fill substrate", body))
