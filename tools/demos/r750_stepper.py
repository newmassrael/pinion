#!/usr/bin/env python3
"""R750 §5.38 §5.40 horizontal stepper (progress steps).

`hello-stepper` is another standalone consumer of the R51.44
`RadioGroupExternal` coordinator (current step = 1-of-N selection) and the
**2nd consumer of the `aria-current` axis**: it exercises
`AriaCurrent::Step`, the variant R731 minted for exactly this widget while
only the breadcrumb's `AriaCurrent::Page` had a consumer. Each step is an
`AriaRole::Button` under an `AriaRole::Group`; the active step carries
`aria-current="step"`. It boots with the first step ("Account") current.

Phase 1 — RPC introspection / behaviour:
  * boot: the first step is current (selected_index == 0); the strip has N
    step indicators + N-1 connectors;
  * the indicator phases render distinctly — the current step's circle
    fills `Accent`, an upcoming step's circle fills
    `SurfaceContainerHighest`, and the glyph is the step number;
  * clicking a step navigates to it: passed steps become *completed*
    (check glyph, `Accent` fill), the clicked step becomes *current*, and
    the connectors leading into reached steps turn `Accent`;
  * keyboard (focus the single tab stop, ArrowRight/Left wrap, Home/End)
    drives the same statechart.
  The `aria-current="step"` mapping on the current `button` is unit-tested
  in the binding (access_node); RPC verifies the underlying selection.

Phase 2 — live-pixel (boot frame): a headless `PINION_SCREENSHOT` capture
confirms the GPU raster matches the structural phase tones — the current
step's circle centre is `Accent`, an upcoming step's circle centre is
`SurfaceContainerHighest` (the two are distinct on real pixels, not just in
the from:paint snapshot). Circle centres are computed from the step
containers' laid-out rects (the boot layout is deterministic and identical
across the RPC subprocess and the screenshot process).

Run from the workspace root:
    cargo build -p hello-stepper --release
    python3 tools/demos/r750_stepper.py
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

EXAMPLE = "hello-stepper"
GROUP = "stepper"
VIEWPORT = (620, 120)
N = 4
CHECK = "✓"


def circle_fill(snap, i):
    """`(r, g, b)` of step `i`'s indicator circle fill."""
    step = find_by_tag(snap, f"{GROUP}#{i}")
    assert step is not None, f"step {i} present"
    circle = step["children"][0]
    fill = circle["style"]["fill"]
    return (fill["r"], fill["g"], fill["b"])


def step_glyph(snap, i):
    """The text content of step `i`'s indicator (number or check)."""
    step = find_by_tag(snap, f"{GROUP}#{i}")
    assert step is not None, f"step {i} present"
    return step["children"][0]["children"][0]["content"]


def connector_count(snap):
    strip = find_by_tag(snap, GROUP)
    assert strip is not None, "stepper strip present"
    # Connectors are the tagless Box children of the strip (steps are
    # tagged Containers); count the bare Boxes.
    return sum(1 for ch in strip.get("children") or [] if ch.get("type") == "Box")


def circle_fill_point(rects, i):
    """A point inside step `i`'s circle that lies on the *fill* (not the
    centred number/check glyph): the circle sits at the left of the step
    row (CIRCLE wide = the row height), so the centre is `(x + h/2, y +
    h/2)`; sampling 11px above the centre clears the glyph while staying
    inside the 16px-radius circle."""
    x, y, _w, h = rects[f"{GROUP}#{i}"]
    return (x + h // 2, y + h // 2 - 11)


def body() -> None:
    centers: dict[int, tuple[int, int]] = {}
    surface_rgb = (0, 0, 0)
    accent = (0, 0, 0)
    upcoming = (0, 0, 0)

    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as d:
        # ── boot: the first step is current ───────────────────────────
        assert_eq(d.query("/external/selected_index"), 0, "boot current = Account (0)")
        assert_eq(d.query("/external/selected.0"), True, "step 0 is current")
        assert_eq(d.query("/external/selected.1"), False, "step 1 is not current")
        assert_eq(d.query("/external/selected.3"), False, "step 3 is not current")

        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GROUP) is not None, "group present"
        for i in range(N):
            assert find_by_tag(snap, f"{GROUP}#{i}") is not None, f"step {i} present"
        assert_eq(connector_count(snap), N - 1, "N-1 connectors")

        # Phase tones at boot: step 0 current (Accent), steps 1..3 upcoming
        # (SurfaceContainerHighest). The number glyph shows on every step
        # (no completed steps yet).
        accent = circle_fill(snap, 0)
        upcoming = circle_fill(snap, 2)
        assert accent != upcoming, "current vs upcoming circle fills differ"
        assert circle_fill(snap, 1) == upcoming == circle_fill(snap, 3), \
            "all upcoming steps share one fill"
        for i in range(N):
            assert_eq(step_glyph(snap, i), str(i + 1), f"step {i} shows its number")

        # Capture the laid-out circle centres for the pixel phase + the
        # surface tone for the background check.
        rects = abs_rects_of(snap)
        centers = {0: circle_fill_point(rects, 0), 2: circle_fill_point(rects, 2)}
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

        # ── click step 2 -> navigate; phases + connectors update ──────
        d.click(path=f"{GROUP}#2")
        d.pointer_leave()
        assert_eq(d.query("/external/selected_index"), 2, "clicking step 2 navigates there")
        assert_eq(d.query("/external/selected.2"), True, "step 2 now current")
        assert_eq(d.query("/external/selected.0"), False, "step 0 no longer current")

        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        # Steps 0,1 completed (check glyph, Accent fill); 2 current
        # (number, Accent); 3 upcoming (number, SurfaceContainerHighest).
        assert_eq(step_glyph(snap, 0), CHECK, "completed step 0 shows a check")
        assert_eq(step_glyph(snap, 1), CHECK, "completed step 1 shows a check")
        assert_eq(step_glyph(snap, 2), "3", "current step 2 shows its number")
        assert_eq(step_glyph(snap, 3), "4", "upcoming step 3 shows its number")
        assert_eq(circle_fill(snap, 0), accent, "completed step 0 fills Accent")
        assert_eq(circle_fill(snap, 1), accent, "completed step 1 fills Accent")
        assert_eq(circle_fill(snap, 2), accent, "current step 2 fills Accent")
        assert_eq(circle_fill(snap, 3), upcoming, "upcoming step 3 stays SurfaceContainerHighest")

        # Connectors leading into reached steps (1 and 2) are now Accent;
        # the last connector (into the upcoming step 3) is not.
        strip = find_by_tag(snap, GROUP)
        boxes = [ch for ch in strip["children"] if ch.get("type") == "Box"]
        assert_eq(len(boxes), N - 1, "still N-1 connectors")
        conn_done = (boxes[0]["style"]["fill"], boxes[1]["style"]["fill"])
        for fill in conn_done:
            assert (fill["r"], fill["g"], fill["b"]) == accent, "reached connectors are Accent"
        last = boxes[2]["style"]["fill"]
        assert (last["r"], last["g"], last["b"]) != accent, "connector into upcoming step is not Accent"

        # ── keyboard navigation (single tab stop + roving) ────────────
        focused = d.request("focus/set", {"tag": GROUP}).result.get("focused")
        assert_eq(focused, GROUP, "strip is the single tab stop")
        d.key(path=GROUP, name="ArrowRight")
        assert_eq(d.query("/external/selected_index"), 3, "ArrowRight 2 -> 3")
        d.key(path=GROUP, name="ArrowRight")
        assert_eq(d.query("/external/selected_index"), 0, "ArrowRight wraps 3 -> 0")
        d.key(path=GROUP, name="ArrowLeft")
        assert_eq(d.query("/external/selected_index"), 3, "ArrowLeft wraps 0 -> 3")
        d.key(path=GROUP, name="Home")
        assert_eq(d.query("/external/selected_index"), 0, "Home -> first step")
        d.key(path=GROUP, name="End")
        assert_eq(d.query("/external/selected_index"), 3, "End -> last step")

    # ── Phase 2 — live-pixel: the boot indicator tones raster ─────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"
    corner = sample_png_points(png, [(6, 6)])[0]
    assert_pixel_eq(corner, (*surface_rgb, 255), "window Surface (6,6)", tolerance=8)

    cur_px, up_px = sample_png_points(png, [centers[0], centers[2]])
    assert_pixel_eq(cur_px, (*accent, 255), "current step circle = Accent", tolerance=10)
    assert_pixel_eq(up_px, (*upcoming, 255), "upcoming step circle = SurfaceContainerHighest", tolerance=10)
    assert cur_px[:3] != up_px[:3], "current vs upcoming circles differ on real pixels"


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r750-")) / "stepper.png"
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
    sys.exit(run_demo("R750 horizontal stepper", body))
