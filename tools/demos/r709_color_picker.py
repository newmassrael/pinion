#!/usr/bin/env python3
"""R709 §5.38 ColorPicker demo — RPC introspection + live-pixel.

The first real consumer of the R708 gradient-fill substrate and the first
consumer of the new §5.38 `ColorArea` 2-D pad widget. A canonical HSV
picker = a 2-D saturation/value pad (the `ColorArea`, painted by layering
a white->transparent and a transparent->black gradient over a pure-hue
base) plus a 1-D hue bar (a `SliderExternal` painted with the rainbow
gradient). The live preview swatch is `Color::from_hsv(h, s, v)`.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, no OCR). The SV pad reads back as a
    solid pure-hue base plus two alpha-bearing linear gradient overlays
    (horizontal saturation white->transparent, vertical value
    transparent->black); the hue bar reads back as the 7-stop rainbow.

  Phase 2 — DRIVE (scene/intervene + scene/drag + scene/key). An agent
    sets the hue / saturation / value axes through the §5.15 introspect
    channel and reads them back, and observes the SV base fill + preview
    swatch re-compute through `Color::from_hsv` — driving the picker with
    zero pixels.

  Phase 3 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame's SV square is sampled at its corners: top-left white
    (saturation 0), top-right the pure hue (saturation 1, value 1), the
    bottom black (value 0) — proving the gradient layering rasterizes —
    and the hue bar at its primary-stop offsets.

Run from the workspace root:
    cargo build -p hello-color-picker --release
    python3 tools/demos/r709_color_picker.py
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

VIEWPORT = (380, 360)

SV_TAG = "sv_pad"
HUE_TAG = "hue_bar"

# Hue-bar stop ramp — mirrors the `const`s in
# `examples/hello-color-picker/src/main.rs` (SSOT for both sides).
HUE_STOPS = [
    (0.0, (0xFF, 0x00, 0x00)),        # red
    (1.0 / 6.0, (0xFF, 0xFF, 0x00)),  # yellow
    (2.0 / 6.0, (0x00, 0xFF, 0x00)),  # green
    (3.0 / 6.0, (0x00, 0xFF, 0xFF)),  # cyan
    (4.0 / 6.0, (0x00, 0x00, 0xFF)),  # blue
    (5.0 / 6.0, (0xFF, 0x00, 0xFF)),  # magenta
    (1.0, (0xFF, 0x00, 0x00)),        # red
]


def hsv_to_rgb(h_deg: float, s: float, v: float) -> tuple[int, int, int]:
    """Mirror of `Color::from_hsv` (the Rust SSOT) for predicting the
    base fill / preview swatch on the Python side."""
    h = h_deg % 360.0
    c = v * s
    hp = (h / 60.0) % 6.0
    x = c * (1.0 - abs((hp % 2.0) - 1.0))
    m = v - c
    sextant = int(hp)
    r1, g1, b1 = [
        (c, x, 0.0),
        (x, c, 0.0),
        (0.0, c, x),
        (0.0, x, c),
        (x, 0.0, c),
        (c, 0.0, x),
    ][min(sextant, 5)]
    return (
        round((r1 + m) * 255.0),
        round((g1 + m) * 255.0),
        round((b1 + m) * 255.0),
    )


def _color(node_color: dict) -> tuple[int, int, int]:
    return (node_color["r"], node_color["g"], node_color["b"])


def _color_rgba(node_color: dict) -> tuple[int, int, int, int]:
    return (node_color["r"], node_color["g"], node_color["b"], node_color["a"])


def assert_color(actual: dict, expected: tuple, label: str) -> None:
    got = _color_rgba(actual) if len(expected) == 4 else _color(actual)
    assert got == expected, f"{label}: color {got} != expected {expected}"


def _approx(actual: float, expected: float, label: str, eps: float = 1e-3) -> None:
    assert abs(actual - expected) < eps, f"{label}: {actual} != {expected}"


def _gradient_of(node: dict, label: str) -> dict:
    grad = node.get("style", {}).get("gradient")
    assert isinstance(grad, dict), f"{label} carries a gradient (not null)"
    return grad


def capture_screenshot() -> Path:
    """Render hello-color-picker's boot frame to RGBA8 PNG via
    PINION_SCREENSHOT (producer parity: same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r709-")) / "picker.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-color-picker"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-color-picker", "--quiet", "--release",
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
    sv_rect = None
    hue_rect = None
    with RpcSubprocess("hello-color-picker") as d:
        # ── Phase 1 — structural introspection ───────────────────────
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        sv = find_by_tag(snap, SV_TAG)
        assert sv is not None, "sv_pad present in paint snapshot"
        # Boot hue = 0 (red), so the SV base fill is the pure red hue and
        # the overlays carve saturation/value out of it.
        assert_color(sv["style"]["fill"], (255, 0, 0), "sv base fill = pure red")
        assert sv["style"].get("gradient") is None, "sv base is a solid fill"

        overlays = [c for c in sv["children"] if c.get("style", {}).get("gradient")]
        assert_eq(len(overlays), 2, "sv pad has two gradient overlays")

        # Saturation overlay — horizontal white opaque -> white transparent.
        sat = _gradient_of(overlays[0], "saturation overlay")
        assert_eq(sat["geometry"]["kind"], "linear", "saturation overlay kind")
        _approx(sat["geometry"]["start"]["u"], 0.0, "saturation start.u")
        _approx(sat["geometry"]["end"]["u"], 1.0, "saturation end.u")
        _approx(sat["geometry"]["end"]["v"], 0.0, "saturation end.v (horizontal)")
        assert_eq(len(sat["stops"]), 2, "saturation stop count")
        assert_color(sat["stops"][0]["color"], (255, 255, 255, 255), "saturation stop0 = opaque white")
        assert_color(sat["stops"][1]["color"], (255, 255, 255, 0), "saturation stop1 = transparent white")

        # Value overlay — vertical black transparent (top) -> black opaque.
        val = _gradient_of(overlays[1], "value overlay")
        assert_eq(val["geometry"]["kind"], "linear", "value overlay kind")
        _approx(val["geometry"]["end"]["u"], 0.0, "value end.u (vertical)")
        _approx(val["geometry"]["end"]["v"], 1.0, "value end.v")
        assert_color(val["stops"][0]["color"], (0, 0, 0, 0), "value stop0 = transparent black")
        assert_color(val["stops"][1]["color"], (0, 0, 0, 255), "value stop1 = opaque black")

        # Hue bar — the 7-stop rainbow.
        hue = find_by_tag(snap, HUE_TAG)
        assert hue is not None, "hue_bar present"
        hg = _gradient_of(hue, "hue bar")
        assert_eq(hg["geometry"]["kind"], "linear", "hue bar geometry kind")
        assert_eq(len(hg["stops"]), 7, "hue bar stop count")
        for i, (off, rgb) in enumerate(HUE_STOPS):
            _approx(hg["stops"][i]["offset"], off, f"hue stop {i} offset")
            assert_color(hg["stops"][i]["color"], rgb, f"hue stop {i} color")

        assert find_by_tag(snap, "preview_swatch") is not None, "preview swatch present"

        rects = abs_rects_of(snap)
        assert SV_TAG in rects and HUE_TAG in rects, "sv pad + hue bar have absolute rects"
        sv_rect = rects[SV_TAG]
        hue_rect = rects[HUE_TAG]

        # Boot axis values: SV centred (0.5, 0.5), hue red (0.0).
        _approx(float(d.query(f"/{SV_TAG}/external/x")), 0.5, "boot saturation")
        _approx(float(d.query(f"/{SV_TAG}/external/y")), 0.5, "boot value")
        _approx(float(d.query(f"/{HUE_TAG}/external/value")), 0.0, "boot hue")
        assert_eq(d.query(f"/{SV_TAG}/external/state"), "Idle", "sv pad state Idle")
        assert_eq(d.query(f"/{HUE_TAG}/external/state"), "Idle", "hue bar state Idle")

        # ── Phase 2 — drive the axes through introspection ───────────
        # Move the hue to green (2/6) -> the SV base fill re-computes to
        # the pure green hue via Color::from_hsv.
        d.intervene(f"/{HUE_TAG}/external/value", 2.0 / 6.0)
        _approx(float(d.query(f"/{HUE_TAG}/external/value")), 2.0 / 6.0, "hue after intervene")
        snap_g = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_color(find_by_tag(snap_g, SV_TAG)["style"]["fill"], (0, 255, 0), "sv base re-keys to pure green")

        # Set saturation 0, value 1 -> selected colour is white regardless
        # of hue; the preview swatch fill must become white.
        d.intervene(f"/{SV_TAG}/external/x", 0.0)
        d.intervene(f"/{SV_TAG}/external/y", 1.0)
        _approx(float(d.query(f"/{SV_TAG}/external/x")), 0.0, "saturation after intervene")
        _approx(float(d.query(f"/{SV_TAG}/external/y")), 1.0, "value after intervene")
        snap_w = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_color(find_by_tag(snap_w, "preview_swatch")["style"]["fill"], (255, 255, 255), "preview = white at S0 V1")

        # Out-of-range intervene clamps (substrate clamp visible in 1 RT).
        d.intervene(f"/{SV_TAG}/external/x", 5.0)
        _approx(float(d.query(f"/{SV_TAG}/external/x")), 1.0, "saturation clamps to 1.0")

        # Pointer drag across the SV pad drives both axes + the drag
        # statechart (Idle/Hover -> Dragging -> Hover).
        d.drag(from_at=(sv_rect[0] + 8, sv_rect[1] + 8),
               to_at=(sv_rect[0] + sv_rect[2] - 8, sv_rect[1] + sv_rect[3] - 8))
        # End of drag near bottom-right: saturation high, value low.
        assert float(d.query(f"/{SV_TAG}/external/x")) > 0.7, "drag raised saturation"
        assert float(d.query(f"/{SV_TAG}/external/y")) < 0.3, "drag lowered value (bottom = dark)"

        # Drag the hue bar to ~the right end (wraps back to red).
        d.drag(from_path=HUE_TAG, to_at=(hue_rect[0] + hue_rect[2] - 4, hue_rect[1] + hue_rect[3] // 2))
        assert float(d.query(f"/{HUE_TAG}/external/value")) > 0.9, "hue drag reached the warm end"

        # Keyboard: focus the SV pad and nudge saturation down with
        # ArrowLeft (W3C slider keyboard via apply_key -> intervene).
        # focus/set is required — a named scene/key dispatches at a
        # coordinate but does not move focus, and apply_key gates on the
        # focused tag.
        d.request("focus/set", {"tag": SV_TAG})
        assert_eq(d.request("focus/get").result["focused"], SV_TAG, "SV pad focused")
        before = float(d.query(f"/{SV_TAG}/external/x"))
        d.key(path=SV_TAG, name="ArrowLeft")
        after = float(d.query(f"/{SV_TAG}/external/x"))
        _approx(after, max(0.0, before - 0.05), "ArrowLeft nudges saturation down by one step")
        # ArrowUp nudges value up on the SV pad.
        vy = float(d.query(f"/{SV_TAG}/external/y"))
        d.key(path=SV_TAG, name="ArrowUp")
        assert float(d.query(f"/{SV_TAG}/external/y")) > vy, "ArrowUp raises value"

    # ── Phase 3 — live-pixel verification of the boot frame ──────────
    assert sv_rect is not None and hue_rect is not None
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, (
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    )

    sx, sy, sw, sh = sv_rect
    # Boot: hue 0 (red), saturation/value centred. SV corners are
    # gradient-only (the thumb is centred, away from every corner).
    corner_pts = {
        "top-left (white, S0 V1)": ((sx + 5, sy + 5), (255, 255, 255)),
        "top-right (pure hue red, S1 V1)": ((sx + sw - 5, sy + 5), (255, 0, 0)),
        "bottom-left (black, V0)": ((sx + 5, sy + sh - 5), (0, 0, 0)),
        "bottom-right (black, V0)": ((sx + sw - 5, sy + sh - 5), (0, 0, 0)),
    }
    pts = [p for p, _ in corner_pts.values()]
    samples = sample_png_points(img, pts)
    for (label, (_pt, rgb)), pixel in zip(corner_pts.items(), samples):
        # Tolerance 32: a 5-px inset lands a hair inside the gradient
        # ramp + Area-AA; the corner colour is unmistakable within it.
        assert_pixel_eq(pixel, (rgb[0], rgb[1], rgb[2], 255),
                        f"SV {label}", tolerance=32)

    # Hue bar at the three interior primary stops (green 1/3, cyan 1/2,
    # blue 2/3) — AT a stop the colour is the stop colour.
    #
    # ★ R1664 — asserted as *where the stop is*, not as *what one nominal pixel
    # holds*. The old form sampled `hx + round(hw * off)` and allowed 24 of
    # slack in the colour, which conflates two different tolerances into one and
    # measures the wrong thing: the hue ramp climbs about 6 per pixel here, so a
    # rasteriser whose gradient origin sits a few pixels over reads as a wrong
    # COLOUR. That is what CI reported — `(0, 255, 26)` at x=83, which is
    # byte-for-byte this host's value at x=87, i.e. the same gradient shifted
    # four pixels, not a different one.
    #
    # So: find the pixel that IS the stop colour (tolerance 8, three times
    # tighter than before) and assert it lands within `STOP_SLOP` of where the
    # bar's geometry says it should. Both halves are now checked, and neither is
    # hostage to a rasteriser's convention for where a gradient begins.
    hx, hy, hw, hh = hue_rect
    row_y = hy + hh // 2
    interior = [HUE_STOPS[2], HUE_STOPS[3], HUE_STOPS[4]]
    STOP_SLOP = 6
    for off, rgb in interior:
        nominal = hx + max(1, min(hw - 2, round(hw * off)))
        xs = [x for x in range(nominal - STOP_SLOP, nominal + STOP_SLOP + 1)
              if hx <= x < hx + hw]
        row = sample_png_points(img, [(x, row_y) for x in xs])
        deltas = [max(abs(px[c] - rgb[c]) for c in range(3)) for px in row]
        best = min(range(len(xs)), key=lambda i: deltas[i])
        assert_pixel_eq(
            row[best], (rgb[0], rgb[1], rgb[2], 255),
            f"hue bar reaches {rgb} somewhere within {STOP_SLOP}px of offset "
            f"{off:.3f} (best was x={xs[best]}, nominal {nominal})",
            tolerance=8,
        )
        assert abs(xs[best] - nominal) <= STOP_SLOP, (
            f"the {rgb} stop is painted at x={xs[best]} and the bar's geometry "
            f"puts offset {off:.3f} at x={nominal}"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R709 ColorPicker (ColorArea + gradient)", body))
