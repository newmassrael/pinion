#!/usr/bin/env python3
"""R1354 Phase-B dataviz substrate demo — RPC introspection + live-pixel.

The first `pinion_chart::LineChart` paint, plus the R1354 slice-1 scrub
inspector. The chart is assembled from retained primitives the paint
adapter already rasterizes: stroked `Scene::Path` polylines (series /
gridlines / axes / crosshair), filled `Scene::Path` (area fills + Bézier
marker dots), `Scene::Text` (tick / tooltip labels), `Scene::Box` (legend
swatches, tooltip card). This demo proves the assembly AND the interactive
scrub end-to-end in the real windowed shell.

Two verifications, per [[ai-first-rpc-introspection-obligation]] +
[[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE + INTERACTION (scene/snapshot). Every chart element
    reads back as tagged data: three 12-point series in the fixed
    Okabe-Ito hues, their area fills, axes, gridlines, per-series legend,
    and the inspect overlay (crosshair / markers / tooltip). Driving the
    scrub value over `scene/intervene` MOVES the crosshair (a robust wire
    proof of pointer -> inspect-position -> nearest-point -> overlay).

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    Series 0's polyline bounding box (from its own snapshot command
    stream) is scanned in the boot frame for its fixed blue (#0072B2); a
    regression that dropped chart paint would leave the box empty. Region
    scan -> theme-independent + anti-alias robust.

Run from the workspace root:
    cargo build -p hello-chart --release
    python3 tools/demos/hello_chart_r1354.py
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
    find_by_tag,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

VIEWPORT = (760, 460)

# The dual-theme Okabe-Ito series palette (SSOT: crates/pinion-chart/
# src/palette.rs OKABE_ITO). Theme-independent.
SERIES_COLORS = [(0x00, 0x72, 0xB2), (0xE6, 0x9F, 0x00), (0x00, 0x9E, 0x73)]
SERIES_NAMES = ["ingress", "egress", "errors"]


def _node(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in paint snapshot"
    return node


def _path(snap, tag: str) -> dict:
    node = _node(snap, tag)
    assert_eq(node["type"], "Path", f"{tag} is a path node")
    return node


def _cmd_types(node: dict) -> list[str]:
    return [c["type"] for c in node["commands"]]


def _points(node: dict) -> list[tuple[float, float]]:
    return [
        (c["point"]["x"], c["point"]["y"]) for c in node["commands"] if "point" in c
    ]


def assert_color(actual: dict, expected: tuple[int, int, int], label: str) -> None:
    got = (actual["r"], actual["g"], actual["b"])
    assert got == expected, f"{label}: color {got} != expected {expected}"


def body() -> None:
    # ── Phase 1 — structure + interaction ────────────────────────────
    with RpcSubprocess("hello-chart") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)

        assert find_by_tag(snap, "chart") is not None, "chart container present"

        # Three series: 12-point stroke-only polylines in fixed hues, each
        # with a filled + closed area fill (area chart default).
        for i, color in enumerate(SERIES_COLORS):
            s = _path(snap, f"chart.series.{i}")
            types = _cmd_types(s)
            assert_eq(len(types), 12, f"series {i}: MoveTo + 11 LineTo")
            assert_eq(types[0], "MoveTo", f"series {i} starts with MoveTo")
            assert s["style"]["fill"] is None, f"series {i} line has no fill"
            stroke = s["style"]["stroke"]
            assert stroke is not None, f"series {i} carries a stroke"
            assert_color(stroke["color"], color, f"series {i} stroke colour")

            area = _path(snap, f"chart.area.{i}")
            assert_eq(_cmd_types(area)[-1], "Close", f"area {i} is closed")
            assert area["style"]["fill"] is not None, f"area {i} is filled"

        # Axes + gridlines.
        for axis in ("chart.axis.x", "chart.axis.y"):
            assert _path(snap, axis)["style"]["stroke"] is not None, f"{axis} stroked"
        assert find_by_tag(snap, "chart.grid.y.0") is not None, "a y gridline"
        assert find_by_tag(snap, "chart.grid.x.0") is not None, "an x gridline"

        # Legend: one swatch (correct hue) + one label (text) per series.
        for i, (color, name) in enumerate(zip(SERIES_COLORS, SERIES_NAMES)):
            swatch = _node(snap, f"chart.legend.{i}.swatch")
            assert_eq(swatch["type"], "Box", f"legend {i} swatch is a box")
            assert_color(swatch["style"]["fill"], color, f"legend {i} swatch hue")
            label = _node(snap, f"chart.legend.{i}.label")
            assert_eq(label.get("content"), name, f"legend {i} label text")

        # Inspect overlay (boot scrub value 0.5): crosshair + tooltip +
        # header + one marker & value line per series.
        assert find_by_tag(snap, "chart.inspect.crosshair") is not None, "crosshair"
        assert find_by_tag(snap, "chart.inspect.tooltip") is not None, "tooltip card"
        assert find_by_tag(snap, "chart.inspect.header") is not None, "tooltip header"
        for i in range(3):
            assert find_by_tag(snap, f"chart.inspect.marker.{i}") is not None, f"marker {i}"
            assert find_by_tag(snap, f"chart.inspect.value.{i}") is not None, f"value {i}"

        crosshair_boot = _points(_path(snap, "chart.inspect.crosshair"))[0][0]

        # ── Drive the scrub over RPC: the crosshair must move right ───
        d.intervene("/external/value", 0.9)
        moved = d.query("/external/value")
        assert abs(moved - 0.9) < 0.02, f"scrub value set to 0.9, got {moved}"
        snap2 = d.snapshot(source="paint", viewport=VIEWPORT)
        crosshair_moved = _points(_path(snap2, "chart.inspect.crosshair"))[0][0]
        assert crosshair_moved > crosshair_boot + 20, (
            f"crosshair moved right with the scrub: {crosshair_boot} -> {crosshair_moved}"
        )
        # Near the right edge the inspector snaps to the nearest bucket.
        # The x domain is the data extent 0..11 (R1357 pins it from the
        # brush), so scrub 0.9 -> data x ~10.05 -> bucket 10 (ingress 2600).
        header = _node(snap2, "chart.inspect.header").get("content")
        assert_eq(header, "x = 10", "tooltip header near the right edge")
        ingress_val = _node(snap2, "chart.inspect.value.0").get("content")
        assert ingress_val is not None and "2.6k" in ingress_val, (
            f"ingress value at x=10 (~2600): {ingress_val!r}"
        )

        # Series 0's boot polyline bbox for the pixel phase.
        series0_points = _points(_path(snap, "chart.series.0"))

        # ── Brush zoom (R1357): the sibling RangeSlider external ──────
        assert find_by_tag(snap, "chart_brush") is not None, "brush strip present"
        x_label_full = _node(snap, "chart.label.x.0").get("content")
        d.intervene("/chart_brush/external/low", 0.30)
        d.intervene("/chart_brush/external/high", 0.62)
        # `intervene` only asserts a response arrived; round-trip the value so
        # a no-op write cannot leave the assertions below vacuously green.
        assert abs(d.query("/chart_brush/external/low") - 0.30) < 0.02, "brush low landed"
        assert abs(d.query("/chart_brush/external/high") - 0.62) < 0.02, "brush high landed"
        snap_zoom = d.snapshot(source="paint", viewport=VIEWPORT)

        # The x axis re-domains: its first tick label must change.
        x_label_zoom = _node(snap_zoom, "chart.label.x.0").get("content")
        assert x_label_full is not None and x_label_zoom is not None, "x tick labels carry text"
        assert x_label_full != x_label_zoom, (
            f"x axis re-domained on brush: {x_label_full!r} -> {x_label_zoom!r}"
        )

        # R1356 clipping: every zoomed vertex stays inside the plot. The
        # plot spans CHART_RECT (14, w=732) minus the default 52/16
        # margins -> x in [66, 730]. Without clipping the pinned domain
        # extrapolates far outside it.
        for i in range(3):
            for x, _y in _points(_path(snap_zoom, f"chart.series.{i}")):
                assert 65.0 <= x <= 731.0, (
                    f"series {i} vertex x={x} escaped the clipped plot [66,730]"
                )

    # ── Phase 2 — live-pixel witness of the boot frame ───────────────
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")

    xs = [p[0] for p in series0_points]
    ys = [p[1] for p in series0_points]
    x0, y0 = int(min(xs)) - 3, int(min(ys)) - 3
    x1, y1 = int(max(xs)) + 3, int(max(ys)) + 3
    hits = _count_color(img, x0, y0, x1, y1, SERIES_COLORS[0], tolerance=40)
    assert hits >= 30, (
        f"series-0 blue rasterized in its bbox "
        f"({x0},{y0})-({x1},{y1}): only {hits} matching pixels"
    )


def _count_color(img, x0, y0, x1, y1, rgb, tolerance):
    """Pixels within `tolerance` (per channel) of `rgb` in the clamped
    rectangle — a region scan robust to anti-alias and centreline miss."""
    r0, g0, b0 = rgb
    count = 0
    for y in range(max(0, y0), min(img.height, y1 + 1)):
        for x in range(max(0, x0), min(img.width, x1 + 1)):
            r, g, b, _a = png_pixel(img, x, y)
            if (
                abs(r - r0) <= tolerance
                and abs(g - g0) <= tolerance
                and abs(b - b0) <= tolerance
            ):
                count += 1
    return count


def capture_screenshot() -> Path:
    """Render hello-chart's boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit — producer parity with the live window's
    `to_vello_cached` rasterizer."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r1354-")) / "chart.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-chart"
    cmd = (
        [str(binary)]
        if binary.exists()
        else ["cargo", "run", "-p", "hello-chart", "--quiet", "--release"]
    )
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd,
        cwd=WORKSPACE_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
        timeout=120.0,
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
    sys.exit(run_demo("R1354 dataviz chart scrub inspector", body))
