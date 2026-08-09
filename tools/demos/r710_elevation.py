#!/usr/bin/env python3
"""R710 §5.50 drop-shadow substrate demo — RPC introspection + live-pixel.

The first `BoxStyle.shadows` paint. `BoxStyle.shadows` carries a
`Vec<BoxShadow>` (colour, offset, blur, spread — the CSS `box-shadow` /
another retained-mode toolkit `List<BoxShadow>` model) that the Vello `paint_adapter` lowers to
the native `Scene::draw_blurred_rounded_rect` primitive, painted behind
the box.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=30 assertions). Each elevation
    card's shadow list is read back as data — the key + ambient pair,
    their blur / offset / colour / spread — with no OCR (§2 #7). The key
    blur rises across the static gallery (level 1 -> 3 -> 5). Toggling
    the raise card lifts it from level 1 to level 5, observed as its
    shadow list growing to match `card_high`'s, proving the Off/On bit
    reaches paint and re-keys the §5.16 paint-cache.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The cast is sampled in the background gap just below each static
    card. A drop-shadow darkens the white surface, and the darkening is
    monotone with elevation: card_high casts a clearly darker gap than
    card_mid, which is darker than the (near-imperceptible) card_low and
    the clean far background. A regression that dropped the shadows would
    leave every gap pure white and these asserts would fail loudly. The
    headless screenshot uses the same `to_vello_cached` rasterizer the
    live window does (producer parity), so a pixel here is a faithful
    witness.

Run from the workspace root:
    cargo build -p hello-elevation --release
    python3 tools/demos/r710_elevation.py
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
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

VIEWPORT = (360, 440)

# Mirrors `KEY_SHADOW` / `AMBIENT_SHADOW` alpha in
# `examples/hello-elevation/src/main.rs` (SSOT for both sides).
KEY_ALPHA = 0x4D     # black @ ~30 %
AMBIENT_ALPHA = 0x26  # black @ ~15 %

# Static gallery: tag -> elevation level (mirrors the `LEVEL_*` consts).
STATIC_LEVELS = {"card_low": 1, "card_mid": 3, "card_high": 5}
RAISED_LEVEL = 5


def expected_shadows(level: int) -> list[dict]:
    """Mirror of `m3_elevation(level)` in the binding — the key + ambient
    pair the demo asserts against."""
    l = float(level)
    return [
        {"blur": l * 1.5, "off_y": l, "alpha": KEY_ALPHA},
        {"blur": l * 3.0, "off_y": l * 0.5, "alpha": AMBIENT_ALPHA},
    ]


def _approx(actual: float, expected: float, label: str, eps: float = 1e-4) -> None:
    assert abs(actual - expected) < eps, f"{label}: {actual} != {expected}"


def _shadows_of(snap, tag: str) -> list[dict]:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in paint snapshot"
    style = node.get("style")
    assert isinstance(style, dict), f"{tag} carries a style object"
    shadows = style.get("shadows")
    assert isinstance(shadows, list), f"{tag} carries a shadows list"
    return shadows


def assert_card_elevation(snap, tag: str, level: int) -> None:
    """Read back `tag`'s shadow list and assert it is the key + ambient
    pair for `level` — colour (alpha), blur, vertical offset, spread."""
    shadows = _shadows_of(snap, tag)
    want = expected_shadows(level)
    assert_eq(len(shadows), 2, f"{tag} casts a key + ambient pair")
    for i, (got, exp) in enumerate(zip(shadows, want)):
        role = "key" if i == 0 else "ambient"
        _approx(got["blur"], exp["blur"], f"{tag} {role} blur", eps=1e-3)
        _approx(got["offset"]["y"], exp["off_y"], f"{tag} {role} offset.y")
        _approx(got["offset"]["x"], 0.0, f"{tag} {role} offset.x")
        _approx(got["spread"], 0.0, f"{tag} {role} spread")
        assert_eq(got["color"]["a"], exp["alpha"], f"{tag} {role} alpha")
        # Drop-shadows are pure black; only alpha governs darkness.
        assert (got["color"]["r"], got["color"]["g"], got["color"]["b"]) == (0, 0, 0), (
            f"{tag} {role} colour is black"
        )


def capture_screenshot() -> Path:
    """Render hello-elevation's initial (Resting / Idle) frame to RGBA8
    PNG via `PINION_SCREENSHOT`, bypassing winit. Producer parity: same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r710-")) / "elevation.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-elevation"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-elevation", "--quiet", "--release",
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


def _lum(pixel) -> int:
    return pixel[0] + pixel[1] + pixel[2]


def body() -> None:
    # ── Phase 1 — structural introspection ───────────────────────────
    card_rects = None
    with RpcSubprocess("hello-elevation") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        # Static gallery — three rising elevations, each a key + ambient
        # pair read back as data.
        for tag, level in STATIC_LEVELS.items():
            assert_card_elevation(snap, tag, level)

        # The key-shadow blur rises strictly with elevation.
        lo = _shadows_of(snap, "card_low")[0]["blur"]
        mid = _shadows_of(snap, "card_mid")[0]["blur"]
        hi = _shadows_of(snap, "card_high")[0]["blur"]
        assert lo < mid < hi, f"key blur rises with elevation: {lo} {mid} {hi}"

        # Raise card starts at the resting elevation (level 1).
        assert_card_elevation(snap, "main_toggle", 1)
        assert_eq(d.query("/external/state"), "Idle", "initial /external/state")
        assert_eq(d.query("/external/value"), False, "initial /external/value (Resting)")

        # Capture the static cards' absolute rects for the pixel phase
        # (same viewport the headless screenshot renders).
        rects = abs_rects_of(snap)
        for tag in STATIC_LEVELS:
            assert tag in rects, f"{tag} has an absolute rect"
        card_rects = {tag: rects[tag] for tag in STATIC_LEVELS}

        # ── Toggle Resting -> Raised: raise card lifts 1 -> 5 ─────────
        d.click(path="main_toggle")
        assert_eq(d.query("/external/value"), True, "/external/value after click (Raised)")

        snap_on = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_card_elevation(snap_on, "main_toggle", RAISED_LEVEL)
        # The raised card now casts exactly card_high's shadow.
        assert_eq(
            _shadows_of(snap_on, "main_toggle")[0]["blur"],
            _shadows_of(snap_on, "card_high")[0]["blur"],
            "raised card matches card_high key blur",
        )
        # The static gallery is mode-independent: still level 5 at the top.
        assert_card_elevation(snap_on, "card_high", 5)

    # ── Phase 2 — live-pixel verification of the cast ────────────────
    assert card_rects is not None
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, (
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    )

    # Clean far background (top corners, away from any card) is the white
    # surface — the baseline the casts darken from.
    far = sample_png_points(img, [(5, 5), (VIEWPORT[0] - 6, 5)])
    for p in far:
        assert _lum(p) >= 760, f"far background is the clean surface, got {p}"

    # Sample the gap 3 px below each card's bottom-centre.
    sums = {}
    for tag, (x, y, w, h) in card_rects.items():
        cx = x + w // 2
        sample = sample_png_points(img, [(cx, y + h + 3)])[0]
        sums[tag] = _lum(sample)

    far_sum = 765
    # card_high casts a clearly darker gap than the white surface.
    assert sums["card_high"] <= 660, f"card_high casts a visible shadow, lum {sums['card_high']}"
    # Monotone with elevation: high darker than mid darker than background.
    assert sums["card_high"] + 24 <= sums["card_mid"], (
        f"card_mid lighter than card_high: {sums['card_mid']} vs {sums['card_high']}"
    )
    assert sums["card_mid"] + 24 <= far_sum, (
        f"card_mid darker than background: {sums['card_mid']}"
    )
    # The level-1 cast is near-imperceptible (correct Material behaviour);
    # it must be no darker than the mid card (monotonicity holds down the
    # ramp).
    assert sums["card_low"] >= sums["card_mid"], (
        f"card_low fainter than card_mid: {sums['card_low']} vs {sums['card_mid']}"
    )


if __name__ == "__main__":
    sys.exit(run_demo("R710 drop-shadow substrate", body))
