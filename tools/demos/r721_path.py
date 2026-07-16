#!/usr/bin/env python3
"""R721 §5.16 vector-path substrate demo — RPC introspection + live-pixel.

The first `Scene::Path` paint. Before R721 the path node carried a complete
data model (commands / style / cache hash / snapshot serialization) but the
Vello `paint_adapter` dropped it (a documented no-op). R721 lowers the
`Vec<PathCommand>` to a Vello `BezPath` and fills (`style.fill`, non-zero
winding) + strokes (`style.stroke`) it.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=40 assertions). Each of the four
    paths is read back as data — its command stream (MoveTo/LineTo/CurveTo/
    Close) and its style sidecar (fill colour, stroke colour/width/cap) — with
    no OCR (§2 #7). Toggling the demo diamond fill-only -> fill+stroke is
    observed as a PathStyle change in the snapshot, proving the Off/On bit
    reaches paint and that the §5.16 paint-cache will re-key.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render). The
    rendered boot frame is sampled at the triangle interior (fill arm), the
    chevron stroke centreline (stroke arm), the diamond centre (Off fill), and
    a clear background corner. A regression that dropped path paint would
    leave these on the surface colour and the asserts would fail loudly. The
    headless screenshot uses the same `to_vello_cached` rasterizer the live
    window does (producer parity), so a pixel here is a faithful witness.

Run from the workspace root:
    cargo build -p hello-path --release
    python3 tools/demos/r721_path.py
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

# Fixed path colours — mirror the `const`s in
# `examples/hello-path/src/main.rs` (SSOT for both sides).
PATH_BLUE = (0x21, 0x96, 0xF3)
DEMO_ON_FILL = (0xE5, 0x39, 0x35)
DEMO_ON_STROKE = (0x10, 0x10, 0x10)
STROKE_TEAL = (0x00, 0x96, 0x88)
STROKE_W = 8

# Boot-frame (Off / Idle) pixel anchors, in window-absolute device px.
# R1358 made the path COMMANDS rect-relative while these anchors stayed
# put: the migration is pixel-identical by construction (each producer
# rebased its geometry onto the very origin the paint adapter translates
# by), so an unchanged pixel here is the proof that the coordinate-basis
# change moved nothing on screen.
TRI_INTERIOR = (90, 133)        # filled triangle centroid -> PATH_BLUE
CHEVRON_STROKE = (185, 120)     # first chevron segment midpoint -> STROKE_TEAL
DIAMOND_CENTRE = (160, 270)     # diamond interior (Off) -> PATH_BLUE
BG_CORNER = (340, 40)           # clear of every path -> surface white


def _path_of(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in paint snapshot"
    assert_eq(node["type"], "Path", f"{tag} is a path node")
    return node


def assert_color(actual: dict, expected: tuple[int, int, int], label: str) -> None:
    got = (actual["r"], actual["g"], actual["b"])
    assert got == expected, f"{label}: color {got} != expected {expected}"


def _cmd_types(node: dict) -> list[str]:
    return [c["type"] for c in node["commands"]]


def body() -> None:
    # ── Phase 1 — structural introspection ───────────────────────────
    with RpcSubprocess("hello-path") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        # Triangle — closed fill-only polygon.
        tri = _path_of(snap, "tri")
        assert_eq(_cmd_types(tri), ["MoveTo", "LineTo", "LineTo", "Close"],
                  "triangle command stream")
        assert_color(tri["style"]["fill"], PATH_BLUE, "triangle fill")
        assert_eq(tri["style"]["stroke"], None, "triangle has no stroke")
        # R1358 — commands are relative to the node's OWN rect, so the first
        # vertex reads 0-based and `rect.origin + command` is the window px.
        # Asserting the SUM (not merely the new literal) is what makes this a
        # contract test rather than a transcription of the change: a producer
        # that regressed to window-absolute commands would still satisfy the
        # 0-based read only by moving its rect, and the sum would then double
        # the origin and fail. The pixel phase below samples that same sum.
        assert_eq(tri["commands"][0]["point"]["x"], 0.0,
                  "triangle start.x is rect-local")
        assert_eq(tri["commands"][0]["point"]["y"], 80.0,
                  "triangle start.y is rect-local")
        assert_eq(tri["rect"]["x"] + tri["commands"][0]["point"]["x"], 50.0,
                  "triangle start.x in window px = rect.x + command")
        assert_eq(tri["rect"]["y"] + tri["commands"][0]["point"]["y"], 160.0,
                  "triangle start.y in window px = rect.y + command")
        # Every vertex lands inside the node's own box — the property that
        # makes the path movable by layout alone (it is false for absolute
        # commands on any node not at the window origin).
        for i, cmd in enumerate(tri["commands"]):
            if "point" not in cmd:
                continue
            px, py = cmd["point"]["x"], cmd["point"]["y"]
            assert 0.0 <= px <= tri["rect"]["w"], \
                f"triangle vertex {i} x={px} within rect w={tri['rect']['w']}"
            assert 0.0 <= py <= tri["rect"]["h"], \
                f"triangle vertex {i} y={py} within rect h={tri['rect']['h']}"

        # Chevron — open stroke-only polyline, round cap.
        chevron = _path_of(snap, "chevron")
        assert_eq(_cmd_types(chevron), ["MoveTo", "LineTo", "LineTo"],
                  "chevron command stream (no Close)")
        assert_eq(chevron["style"]["fill"], None, "chevron has no fill")
        stroke = chevron["style"]["stroke"]
        assert stroke is not None, "chevron carries a stroke"
        assert_color(stroke["color"], STROKE_TEAL, "chevron stroke colour")
        assert_eq(stroke["width"], STROKE_W, "chevron stroke width")
        assert_eq(stroke["cap"], "Round", "chevron stroke cap")

        # Arc — cubic-Bezier (MoveTo + CurveTo), stroke present.
        arc = _path_of(snap, "arc")
        assert_eq(_cmd_types(arc), ["MoveTo", "CurveTo"], "arc command stream")
        curve = arc["commands"][1]
        assert "c1" in curve and "c2" in curve and "end" in curve, \
            "CurveTo carries c1/c2/end control points"
        assert arc["style"]["stroke"] is not None, "arc carries a stroke"

        # Demo diamond — fill-only while Off.
        diamond = _path_of(snap, "demo_path")
        assert_eq(_cmd_types(diamond),
                  ["MoveTo", "LineTo", "LineTo", "LineTo", "Close"],
                  "diamond command stream")
        assert_color(diamond["style"]["fill"], PATH_BLUE, "diamond fill (Off)")
        assert_eq(diamond["style"]["stroke"], None, "diamond has no stroke (Off)")

        # State before the toggle.
        assert_eq(d.query("/external/state"), "Idle", "initial /external/state")
        assert_eq(d.query("/external/value"), False, "initial /external/value (Off)")

        # ── Toggle Off -> On: diamond gains a stroke + flips fill ─────
        d.click(path="main_toggle")
        assert_eq(d.query("/external/value"), True, "/external/value after click (On)")

        snap_on = d.snapshot(source="paint", viewport=VIEWPORT)
        diamond_on = _path_of(snap_on, "demo_path")
        assert_color(diamond_on["style"]["fill"], DEMO_ON_FILL, "diamond fill (On)")
        stroke_on = diamond_on["style"]["stroke"]
        assert stroke_on is not None, "diamond gains a stroke (On)"
        assert_color(stroke_on["color"], DEMO_ON_STROKE, "diamond stroke colour (On)")

        # The other paths are mode-independent.
        assert_color(_path_of(snap_on, "tri")["style"]["fill"], PATH_BLUE,
                     "triangle stays blue after toggle")
        assert _path_of(snap_on, "chevron")["style"]["stroke"] is not None, \
            "chevron stays stroked after toggle"

    # ── Phase 2 — live-pixel verification of the boot (Off) frame ────
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"

    samples = sample_png_points(img, [TRI_INTERIOR, CHEVRON_STROKE,
                                      DIAMOND_CENTRE, BG_CORNER])
    tri_px, chevron_px, diamond_px, bg_px = samples

    # Fill arm: triangle interior is the solid fill colour.
    assert_pixel_eq(tri_px, (*PATH_BLUE, 255),
                    f"triangle interior {TRI_INTERIOR}", tolerance=24)
    # Stroke arm: a point on the chevron stroke centreline is teal.
    assert_pixel_eq(chevron_px, (*STROKE_TEAL, 255),
                    f"chevron stroke {CHEVRON_STROKE}", tolerance=32)
    # Diamond interior (Off) is the fill colour.
    assert_pixel_eq(diamond_px, (*PATH_BLUE, 255),
                    f"diamond centre (Off) {DIAMOND_CENTRE}", tolerance=24)
    # Background corner is clear of every path — surface white.
    assert_pixel_eq(bg_px, (0xFF, 0xFF, 0xFF, 255),
                    f"background corner {BG_CORNER}", tolerance=8)


def capture_screenshot() -> Path:
    """Render hello-path's initial (Off / Idle) frame to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r721-")) / "path.png"
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
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R721 vector-path substrate", body))
