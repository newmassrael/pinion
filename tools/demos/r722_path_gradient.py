#!/usr/bin/env python3
"""R722 §5.50 gradient-fill path substrate demo — RPC introspection + live-pixel.

`PathStyle.gradient` (R722) carries an optional `Gradient` painted in place of the
solid `fill`, isomorphic to `BoxStyle.gradient` (R708) — the Vello `paint_adapter`
lowers it to a `peniko::Gradient` via the shared `gradient_brush`, only the filled
*shape* (BezPath vs rect) differs. Unblocked by R721 (the first Scene::Path paint).

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=20 assertions). The two gradient paths
    are read back as data — geometry kind (linear / radial), UV endpoints /
    centre+radius, and each stop's offset + colour — with no OCR (§2 #7). The
    path's `style.gradient` is the same wire shape a Box gradient uses.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render). The
    `grad_linear` rect's midpoint is sampled: it is the white mid stop, a colour
    impossible for any solid fill or stroke (the colour AT a stop is the stop
    colour irrespective of interpolation space). Left/right interior columns are
    sampled to prove a real ramp (blue-dominant left, red-dominant right). The
    `grad_radial` diamond's exact centre is the centre stop colour, and an
    off-centre interior point trends toward the edge stop. A regression that
    dropped gradient-on-path would paint the (absent) solid fill = nothing, and
    these asserts would fail loudly.

Run from the workspace root:
    cargo build -p hello-path --release
    python3 tools/demos/r722_path_gradient.py
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
    assert_eq,
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

VIEWPORT = (360, 400)

# Mirror the gradient `const`s in examples/hello-path/src/main.rs (SSOT).
GRAD_A = (0x21, 0x96, 0xF3)   # blue  (linear stop 0, radial edge)
GRAD_B = (0xE5, 0x39, 0x35)   # red   (linear stop 1, radial centre)
GRAD_MID = (0xFF, 0xFF, 0xFF)  # white (linear mid stop 0.5)

# grad_linear rect spans x[45,140] y[168,220]; the horizontal ramp's
# mid stop sits at x = 45 + 0.5*95 = 92.
LIN_MID = (92, 194)     # white mid stop (exact)
LIN_LEFT = (52, 194)    # blue-dominant
LIN_RIGHT = (133, 194)  # red-dominant
# grad_radial diamond centre (305,195); a point toward the right edge.
RAD_CENTRE = (305, 195)  # centre stop = GRAD_B (exact)
RAD_EDGEWARD = (326, 195)  # trends toward edge stop GRAD_A


def _gradient_of(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present"
    assert_eq(node["type"], "Path", f"{tag} is a path node")
    style = node["style"]
    assert style["fill"] is None, f"{tag} has no solid fill (gradient instead)"
    grad = style["gradient"]
    assert isinstance(grad, dict), f"{tag} carries a gradient (not null)"
    return grad


def assert_color(actual: dict, expected: tuple[int, int, int], label: str) -> None:
    got = (actual["r"], actual["g"], actual["b"])
    assert got == expected, f"{label}: color {got} != expected {expected}"


def body() -> None:
    # ── Phase 1 — structural introspection ───────────────────────────
    with RpcSubprocess("hello-path") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        # Linear gradient rect — 3 stops, horizontal.
        lin = _gradient_of(snap, "grad_linear")
        assert_eq(lin["geometry"]["kind"], "linear", "grad_linear kind")
        # horizontal() = start(0,0) -> end(1,0): u varies, v constant.
        assert_eq(lin["geometry"]["start"]["u"], 0.0, "grad_linear start.u")
        assert_eq(lin["geometry"]["end"]["u"], 1.0, "grad_linear end.u")
        assert_eq(lin["geometry"]["start"]["v"], lin["geometry"]["end"]["v"],
                  "grad_linear is horizontal (v constant)")
        stops = lin["stops"]
        assert_eq(len(stops), 3, "grad_linear stop count")
        assert_eq(stops[0]["offset"], 0.0, "grad_linear stop0 offset")
        assert_eq(stops[1]["offset"], 0.5, "grad_linear stop1 offset")
        assert_eq(stops[2]["offset"], 1.0, "grad_linear stop2 offset")
        assert_color(stops[0]["color"], GRAD_A, "grad_linear stop0 colour")
        assert_color(stops[1]["color"], GRAD_MID, "grad_linear stop1 colour")
        assert_color(stops[2]["color"], GRAD_B, "grad_linear stop2 colour")

        # Radial gradient diamond — 2 stops, centred.
        rad = _gradient_of(snap, "grad_radial")
        assert_eq(rad["geometry"]["kind"], "radial", "grad_radial kind")
        assert_eq(rad["geometry"]["center"]["u"], 0.5, "grad_radial center.u")
        assert_eq(rad["geometry"]["center"]["v"], 0.5, "grad_radial center.v")
        assert_eq(rad["geometry"]["radius"], 0.5, "grad_radial radius")
        rstops = rad["stops"]
        assert_eq(len(rstops), 2, "grad_radial stop count")
        assert_color(rstops[0]["color"], GRAD_B, "grad_radial centre stop")
        assert_color(rstops[1]["color"], GRAD_A, "grad_radial edge stop")

    # ── Phase 2 — live-pixel verification ────────────────────────────
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"

    pts = [LIN_MID, LIN_LEFT, LIN_RIGHT, RAD_CENTRE, RAD_EDGEWARD]
    mid, left, right, centre, edgeward = sample_png_points(img, pts)

    # The killer pixel: the linear ramp's white mid stop — impossible for
    # any solid fill or stroke.
    assert_pixel_eq(mid, (*GRAD_MID, 255), f"grad_linear mid stop {LIN_MID}", tolerance=10)
    # A real horizontal ramp: left column blue-dominant, right red-dominant.
    assert left[2] > left[0], f"grad_linear left {LIN_LEFT} blue-dominant (b>r)"
    assert right[0] > right[2], f"grad_linear right {LIN_RIGHT} red-dominant (r>b)"

    # Radial centre is the exact centre stop (red); edgeward trends to blue.
    assert_pixel_eq(centre, (*GRAD_B, 255), f"grad_radial centre {RAD_CENTRE}", tolerance=16)
    assert edgeward[2] > centre[2], \
        f"grad_radial edgeward {RAD_EDGEWARD} bluer than centre (b rises toward edge)"


def capture_screenshot() -> Path:
    """Render hello-path's boot frame to RGBA8 PNG via PINION_SCREENSHOT
    (producer parity: same to_vello_cached rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r722-")) / "path_gradient.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-path"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-path", "--quiet", "--release",
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
    sys.exit(run_demo("R722 gradient-fill path substrate", body))
