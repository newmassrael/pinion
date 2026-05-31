#!/usr/bin/env python3
"""R731 §5.38 §5.40 WAI-ARIA breadcrumb navigation.

`hello-breadcrumb` is the 3rd standalone consumer of the R51.44
`RadioGroupExternal` coordinator (current location = 1-of-N selection),
adding three foundational a11y primitives: a `navigation` landmark, `link`
crumbs, and `aria-current="page"` on the current crumb. It boots with the
last crumb ("Summary") current.

Phase 1 — RPC introspection / behaviour:
  * boot: the last crumb is current (selected_index == 3); the trail has
    N crumbs + N-1 "/" separators;
  * the current crumb's text colour (OnSurface) differs from the link
    crumbs (Accent), and all link crumbs share one colour;
  * clicking a crumb navigates to it (current moves; colours swap);
  * keyboard (focus the single tab stop, ArrowRight/Left wrap, Home/End)
    drives the same statechart.
  The `aria-current="page"` mapping on the current `link` is unit-tested
  in the binding (access_node); RPC verifies the underlying selection.

Phase 2 — live-pixel (boot frame): the Surface background renders (a
breadcrumb is a text widget, so the distinguishing link/current colours
are verified structurally above via snapshot fg_color; the boot pixel is
the surface).

Run from the workspace root:
    cargo build -p hello-breadcrumb --release
    python3 tools/demos/r731_breadcrumb.py
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

EXAMPLE = "hello-breadcrumb"
NAV = "breadcrumb"
VIEWPORT = (600, 120)
N = 4


def crumb_fg(snap, i):
    """`(r, g, b)` of crumb `i`'s text colour."""
    node = find_by_tag(snap, f"{NAV}#{i}")
    assert node is not None, f"crumb {i} present"
    text = node["children"][0]
    fg = text["style"]["fg_color"]
    return (fg["r"], fg["g"], fg["b"])


def separator_count(snap):
    nav = find_by_tag(snap, NAV)
    assert nav is not None, "nav landmark present"
    n = 0

    def walk(node):
        nonlocal n
        if not isinstance(node, dict):
            return
        if node.get("type") == "Text" and node.get("content") == "/":
            n += 1
        for ch in node.get("children") or []:
            walk(ch)

    walk(nav)
    return n


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as d:
        # ── boot: last crumb is current ───────────────────────────────
        assert_eq(d.query("/external/selected_index"), 3, "boot current = Summary (3)")
        assert_eq(d.query("/external/selected.3"), True, "crumb 3 is current")
        assert_eq(d.query("/external/selected.0"), False, "crumb 0 is not current")

        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, NAV) is not None, "navigation landmark present"
        for i in range(N):
            assert find_by_tag(snap, f"{NAV}#{i}") is not None, f"crumb {i} present"
        assert_eq(separator_count(snap), N - 1, "N-1 separators")

        # Current crumb colour (OnSurface) differs from the links (Accent);
        # all three link crumbs share one colour.
        link0, link1, link2 = crumb_fg(snap, 0), crumb_fg(snap, 1), crumb_fg(snap, 2)
        current3 = crumb_fg(snap, 3)
        assert link0 == link1 == link2, "all link crumbs share the Accent colour"
        assert current3 != link0, "the current crumb colour (OnSurface) differs from links"

        # ── click crumb 0 -> navigate; current + colours move ─────────
        d.click(path=f"{NAV}#0")
        d.pointer_leave()
        assert_eq(d.query("/external/selected_index"), 0, "clicking crumb 0 navigates there")
        assert_eq(d.query("/external/selected.0"), True, "crumb 0 now current")
        assert_eq(d.query("/external/selected.3"), False, "crumb 3 no longer current")
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert crumb_fg(snap, 0) != crumb_fg(snap, 3), "current/link colours swapped"
        assert crumb_fg(snap, 0) == current3, "crumb 0 now wears the current colour"

        # ── keyboard navigation (single tab stop + roving) ────────────
        focused = d.request("focus/set", {"tag": NAV}).result.get("focused")
        assert_eq(focused, NAV, "trail is the single tab stop")
        d.key(path=NAV, name="ArrowRight")
        assert_eq(d.query("/external/selected_index"), 1, "ArrowRight 0 -> 1")
        d.key(path=NAV, name="ArrowLeft")
        assert_eq(d.query("/external/selected_index"), 0, "ArrowLeft 1 -> 0")
        d.key(path=NAV, name="ArrowLeft")
        assert_eq(d.query("/external/selected_index"), 3, "ArrowLeft wraps 0 -> 3")
        d.key(path=NAV, name="Home")
        assert_eq(d.query("/external/selected_index"), 0, "Home -> first crumb")
        d.key(path=NAV, name="End")
        assert_eq(d.query("/external/selected_index"), 3, "End -> last crumb")

        # ── surface tone for the pixel phase ──────────────────────────
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

    # ── Phase 2 — live-pixel: the boot Surface renders ────────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"
    corner = sample_png_points(png, [(6, 6)])[0]
    assert_pixel_eq(corner, (*surface_rgb, 255), "window Surface (6,6)", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r731-")) / "breadcrumb.png"
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
    sys.exit(run_demo("R731 WAI-ARIA breadcrumb", body))
