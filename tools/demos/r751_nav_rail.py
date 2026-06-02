#!/usr/bin/env python3
"""R751 §5.38 §5.40 WAI-ARIA navigation rail.

`hello-nav-rail` is the **2nd consumer** of the `Navigation` / `Link` a11y
primitives the breadcrumb (R731) introduced — a vertical `navigation`
landmark of destination `link`s, the active one carrying
`aria-current="page"`. The active destination wears the Material 3
active-indicator pill (a tonal `SurfaceContainerHighest` rounded surface);
inactive destinations are transparent. It reuses the `RadioGroupExternal`
coordinator (active destination = 1-of-N selection) with no new interaction
substrate, and boots with the first destination ("Home") active.

Phase 1 — RPC introspection / behaviour:
  * boot: the first destination is active (selected_index == 0); the rail
    has N destinations;
  * the active destination's pill fill (SurfaceContainerHighest) differs
    from the inactive destinations (transparent), which all share one fill;
  * clicking a destination navigates to it (active + pill move);
  * keyboard (focus the single tab stop, ArrowDown/Up wrap, Home/End)
    drives the same statechart.
  The `aria-current="page"` mapping on the active `link` is unit-tested in
  the binding (access_node); RPC verifies the underlying selection.

Phase 2 — live-pixel (boot frame): a headless `PINION_SCREENSHOT` capture
confirms the active-indicator pill rasters — destination 0's pill area is
SurfaceContainerHighest while an inactive destination's is the Surface
background (distinct on real pixels). Pill rects come from the destination
containers' laid-out rects (deterministic, identical across processes).

Run from the workspace root:
    cargo build -p hello-nav-rail --release
    python3 tools/demos/r751_nav_rail.py
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

EXAMPLE = "hello-nav-rail"
NAV = "nav_rail"
VIEWPORT = (220, 280)
N = 4


def pill_fill(snap, i):
    """`(r, g, b, a)` of destination `i`'s active-indicator pill fill."""
    node = find_by_tag(snap, f"{NAV}#{i}")
    assert node is not None, f"destination {i} present"
    fill = node["style"]["fill"]
    return (fill["r"], fill["g"], fill["b"], fill["a"])


def pill_sample_point(rects, i):
    """A point on destination `i`'s pill, right of the left-aligned label
    (label pad = 16px), vertically centred."""
    x, y, w, h = rects[f"{NAV}#{i}"]
    return (x + w - 12, y + h // 2)


def body() -> None:
    centers: dict[int, tuple[int, int]] = {}
    surface_rgb = (0, 0, 0)
    active_fill = (0, 0, 0, 0)

    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as d:
        # ── boot: the first destination is active ─────────────────────
        assert_eq(d.query("/external/selected_index"), 0, "boot active = Home (0)")
        assert_eq(d.query("/external/selected.0"), True, "destination 0 is active")
        assert_eq(d.query("/external/selected.2"), False, "destination 2 is not active")

        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, NAV) is not None, "navigation landmark present"
        for i in range(N):
            assert find_by_tag(snap, f"{NAV}#{i}") is not None, f"destination {i} present"

        # Active destination's pill is opaque (SurfaceContainerHighest);
        # inactive destinations are transparent and share one fill.
        active_fill = pill_fill(snap, 0)
        inactive1, inactive2, inactive3 = pill_fill(snap, 1), pill_fill(snap, 2), pill_fill(snap, 3)
        assert inactive1 == inactive2 == inactive3, "inactive destinations share one fill"
        assert active_fill != inactive1, "active pill fill differs from inactive"
        assert active_fill[3] == 255, "active pill is opaque"
        assert inactive1[3] == 0, "inactive pill is transparent"

        rects = abs_rects_of(snap)
        centers = {0: pill_sample_point(rects, 0), 2: pill_sample_point(rects, 2)}
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

        # ── click destination 2 -> navigate; active + pill move ───────
        d.click(path=f"{NAV}#2")
        d.pointer_leave()
        assert_eq(d.query("/external/selected_index"), 2, "clicking destination 2 navigates there")
        assert_eq(d.query("/external/selected.2"), True, "destination 2 now active")
        assert_eq(d.query("/external/selected.0"), False, "destination 0 no longer active")
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert pill_fill(snap, 2)[3] == 255, "destination 2 pill now opaque"
        assert pill_fill(snap, 0)[3] == 0, "destination 0 pill now transparent"
        assert_eq(pill_fill(snap, 2), active_fill, "destination 2 wears the active pill fill")

        # ── keyboard navigation (single tab stop + roving) ────────────
        focused = d.request("focus/set", {"tag": NAV}).result.get("focused")
        assert_eq(focused, NAV, "rail is the single tab stop")
        d.key(path=NAV, name="ArrowDown")
        assert_eq(d.query("/external/selected_index"), 3, "ArrowDown 2 -> 3")
        d.key(path=NAV, name="ArrowDown")
        assert_eq(d.query("/external/selected_index"), 0, "ArrowDown wraps 3 -> 0")
        d.key(path=NAV, name="ArrowUp")
        assert_eq(d.query("/external/selected_index"), 3, "ArrowUp wraps 0 -> 3")
        d.key(path=NAV, name="Home")
        assert_eq(d.query("/external/selected_index"), 0, "Home -> first destination")
        d.key(path=NAV, name="End")
        assert_eq(d.query("/external/selected_index"), 3, "End -> last destination")

    # ── Phase 2 — live-pixel: the boot active-indicator pill rasters ──
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"
    active_px, inactive_px = sample_png_points(png, [centers[0], centers[2]])
    assert_pixel_eq(active_px, active_fill[:3] + (255,), "active pill = SurfaceContainerHighest", tolerance=10)
    assert_pixel_eq(inactive_px, (*surface_rgb, 255), "inactive destination = Surface", tolerance=10)
    assert active_px[:3] != inactive_px[:3], "active vs inactive differ on real pixels"


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r751-")) / "nav_rail.png"
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
    sys.exit(run_demo("R751 WAI-ARIA navigation rail", body))
