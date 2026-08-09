#!/usr/bin/env python3
"""R640 §5.7 — the design tool-button-m3 reactive lift dogfood (§5.49 R59).

R635 landed the design tool → pinion binding as a static one-frame snapshot:
the SCXML `ButtonExternal` was wrapped, but `read_state` was clamped to
`Idle` and the view fn ignored its `state` argument. R640 lifts the
binding onto the same reactive substrate `hello-button` carries — this
demo is its AI-first self-verification harness per
`[[ai-first-rpc-introspection-obligation]]`.

Verification arc, mirroring the pre-R640 baseline `hello-button` covers
through the same RPC primitives:

  1. spawn the design tool-button-m3
  2. baseline `scene/query "/external/state"` → `"Idle"`
  3. synthesise full Idle → Hover → Pressed → Hover (activate) cycle
     via `scene/invoke "/external/send"`:
       a. PointerEnter           → state = Hover
       b. PointerDown            → state = Pressed
       c. PointerUp              → state = Hover (Material 3 click
                                  resolution; `Button::detect`
                                  emits a `"click"` intent on this
                                  edge)
       d. PointerLeave           → state = Idle
  4. assert state machine traversed every expected variant
  5. assert `Disable` / `Enable` lifecycle transitions reach the
     disabled state and recover

Each `invoke` returns immediately; `query` after the round-trip
observes the post-transition SCXML state. The R51.169 frame-end intent
drain plus `walk_scene_and_drain` purges the intent batch before our
`scene/intents` check would land, so we observe transitions through
state queries — same convention `hello-toggle activate cycle` follows.

Exit 0 on every assertion satisfied, non-zero on the first mismatch.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    WORKSPACE_ROOT,
    assert_eq,
    assert_pixel_eq,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)


# The design tool spec constants — duplicate of the Rust-side `const`s in
# `examples/the design tool-button-m3/src/main.rs`. Kept here verbatim so the
# 9-point sampler indexes the same pixel positions the spec carries.
WIN_W, WIN_H = 320, 160
BTN_W, BTN_H = 109, 40
BTN_FILL = (103, 80, 164, 255)     # #675AA4 M3 Primary
CANVAS_BG = (0x1F, 0x1F, 0x1F, 255)  # neutral dark


def capture_idle_screenshot() -> Path:
    """Run figma-button-m3 with `PINION_SCREENSHOT=<tmp>` to bypass winit
    and write the initial Idle paint scene as RGBA8 PNG. Returns the
    captured path; the file lives in `tempfile.gettempdir()` so CI
    cleanup picks it up.
    """
    out = Path(tempfile.mkdtemp(prefix="pinion-r640-")) / "btn.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "figma-button-m3"
    if binary.exists():
        cmd = [str(binary)]
    else:
        cmd = ["cargo", "run", "-p", "figma-button-m3", "--quiet", "--release"]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd,
        cwd=WORKSPACE_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
        timeout=60.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stdout: {res.stdout!r}\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


def verify_idle_pixels() -> None:
    """R640 §5.7 + [[center-only-pixel-sample-anti-pattern]] — 9-point
    pixel sample of the Idle paint scene.

    Geometry: 320x160 canvas with a centred 109x40 button at
    `[105.5, 60] .. [214.5, 100]` (button rect), corner-radius 100
    (clamped to `min(w, h) / 2 = 20` inside `paint_adapter` per R639).
    The clamp makes the rendering a pill — the four corners of the
    rect-bounding-box are CANVAS, not BUTTON, which is the R639 lesson
    a center-only sampler would have missed.

      ┌───────────────────────────┐
      │   c1 ─── em-top ─── c2    │   ← canvas at corners,
      │                           │      fill at edge midpoints
      │  em-l  CENTER  em-r       │
      │                           │
      │   c3 ── em-bot ── c4      │
      └───────────────────────────┘

    Tolerance = 4 to accommodate the Area-AA half-pixel coverage the
    wgpu + vello pipeline emits at the pill cap boundaries. Bit-exact
    asserts at the canvas centre and at the deep button interior.
    """
    png_path = capture_idle_screenshot()
    png = read_png_rgba8(png_path)
    if (png.width, png.height) != (WIN_W, WIN_H):
        raise AssertionError(
            f"screenshot {png.width}x{png.height} != expected {WIN_W}x{WIN_H}"
        )

    # Button rect (centred): left = 106, right = 214, top = 60, bottom = 99.
    # Interior samples + edge midpoints.
    center = (WIN_W // 2, WIN_H // 2)                       # (160, 80)
    edge_left = (center[0] - BTN_W // 2 + 8, center[1])      # deep-left, inside pill
    edge_right = (center[0] + BTN_W // 2 - 8, center[1])     # deep-right, inside pill
    edge_top = (center[0], center[1] - BTN_H // 2 + 4)       # near top, inside fill
    edge_bot = (center[0], center[1] + BTN_H // 2 - 4)       # near bottom, inside fill

    # Corner-of-bbox samples — these are OUTSIDE the rounded pill, so
    # they must read as canvas. R639 was the regression these would
    # have caught: corner_radius wasn't wired through, the button
    # painted as a sharp rectangle, and these four corners read as
    # BUTTON_FILL instead of CANVAS_BG.
    bbox_corners = [
        (center[0] - BTN_W // 2, center[1] - BTN_H // 2),   # top-left
        (center[0] + BTN_W // 2 - 1, center[1] - BTN_H // 2),  # top-right
        (center[0] - BTN_W // 2, center[1] + BTN_H // 2 - 1),  # bottom-left
        (center[0] + BTN_W // 2 - 1, center[1] + BTN_H // 2 - 1),  # bottom-right
    ]

    # Far-canvas sample — well away from the button, must be exactly
    # CANVAS_BG (no AA bleed).
    far_canvas = (16, 16)

    interior_samples = sample_png_points(
        png, [center, edge_left, edge_right, edge_top, edge_bot]
    )
    for label, (x, y), pixel in zip(
        ["center", "edge-left", "edge-right", "edge-top", "edge-bot"],
        [center, edge_left, edge_right, edge_top, edge_bot],
        interior_samples,
    ):
        assert_pixel_eq(
            pixel,
            BTN_FILL,
            f"button interior {label} ({x}, {y})",
            tolerance=4,
        )

    corner_samples = sample_png_points(png, bbox_corners)
    for label, (x, y), pixel in zip(
        ["bbox-top-left", "bbox-top-right", "bbox-bottom-left", "bbox-bottom-right"],
        bbox_corners,
        corner_samples,
    ):
        # Tolerance = 6 because the pill cap arc grazes very close to
        # the bbox corner at radius = h/2 (the pill perfectly fills the
        # vertical span). A few-byte band accommodates the AA
        # coverage gradient at the arc tangent.
        assert_pixel_eq(
            pixel,
            CANVAS_BG,
            f"corner-of-bbox {label} ({x}, {y}) must read as canvas — "
            "this is the R639 regression sentinel",
            tolerance=6,
        )

    far = sample_png_points(png, [far_canvas])[0]
    assert_pixel_eq(far, CANVAS_BG, f"far canvas {far_canvas}", tolerance=0)


def body() -> None:
    # ── Phase 1 — pixel-level Idle verification ──────────────────
    verify_idle_pixels()

    # ── Phase 2 — RPC-driven state machine introspection ─────────
    with RpcSubprocess("figma-button-m3") as btn:
        initial = btn.query("/external/state")
        assert_eq(initial, "Idle", "initial /external/state")

        # ── Idle → Hover → Pressed → Hover (activate) → Idle ──────
        btn.invoke("/external/send", "PointerEnter")
        assert_eq(
            btn.query("/external/state"),
            "Hover",
            "/external/state after PointerEnter",
        )

        btn.invoke("/external/send", "PointerDown")
        assert_eq(
            btn.query("/external/state"),
            "Pressed",
            "/external/state after PointerDown",
        )

        btn.invoke("/external/send", "PointerUp")
        assert_eq(
            btn.query("/external/state"),
            "Hover",
            "/external/state after PointerUp (Material 3 click resolves to Hover)",
        )

        btn.invoke("/external/send", "PointerLeave")
        assert_eq(
            btn.query("/external/state"),
            "Idle",
            "/external/state after PointerLeave",
        )

        # ── Disable / Enable lifecycle ────────────────────────────
        btn.invoke("/external/send", "Disable")
        assert_eq(
            btn.query("/external/state"),
            "Disabled",
            "/external/state after Disable",
        )

        btn.invoke("/external/send", "Enable")
        assert_eq(
            btn.query("/external/state"),
            "Idle",
            "/external/state after Enable returns to Idle",
        )


if __name__ == "__main__":
    sys.exit(run_demo("figma-button-m3 reactive lift", body))
