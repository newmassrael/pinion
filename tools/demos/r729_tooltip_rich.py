#!/usr/bin/env python3
"""R729 §5.38 §5.40 §5.50 Material 3 rich tooltip.

`hello-tooltip-rich` is the 2nd standalone consumer of the R695
`TooltipExternal` visibility statechart and the 5th consumer of the R711
`elevation` ramp — substrate-0. Unlike the plain tooltip (MD3 Level 0,
flat inverseSurface), the rich tooltip is an **elevated `surfaceContainer`
surface carrying a title + a supporting paragraph**, clearing the
R711/R723/R724 elevated-tooltip carry.

Phase 1 — RPC introspection / behaviour:
  * boot: hidden (no `autosave#pop` overlay);
  * hover the trigger -> shown; the overlay is an elevated surfaceContainer
    (shadows = the elevation(2) key+ambient pair) distinct from both the
    window Surface and the trigger's SurfaceContainerHighest, with a 12 px
    radius and a title + supporting-paragraph text pair;
  * Escape dismisses while hover stays (WCAG 1.4.13 dismissible);
  * leave + re-hover re-shows (the dismiss latch clears on the episode
    falling edge);
  * keyboard focus shows it (WCAG 1.4.13 focus trigger), Escape dismisses.

Phase 2 — live-pixel (boot frame):
  * the trigger chip renders its SurfaceContainerHighest tone (read from
    the paint snapshot — introspection<->screen parity).
  The rich overlay is hover-only, so it is NOT in the boot frame (the
  R711 dialog / R725 snackbar overlay limitation); its surface is verified
  structurally above, and the shadow rasterization is already pixel-proven
  by hello-elevation (R710) + the surfaceContainer tone by R723/R728.

Run from the workspace root:
    cargo build -p hello-tooltip-rich --release
    python3 tools/demos/r729_tooltip_rich.py
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

EXAMPLE = "hello-tooltip-rich"
VIEWPORT = (480, 320)
TRIGGER = "autosave"
POP = "autosave#pop"
TITLE = "Auto-save"
BODY = "Saves your changes automatically every few seconds, so you never lose work."
# elevation(2) key+ambient ramp (mirrors pinion_widget_paint::elevation).
KEY = {"blur": 3.0, "off_y": 2.0, "alpha": 0x4D}
AMBIENT = {"blur": 6.0, "off_y": 1.0, "alpha": 0x26}


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def rgba(fill):
    return (fill["r"], fill["g"], fill["b"], fill.get("a", 255))


def _approx(a, e, label, eps=1e-3):
    assert abs(float(a) - float(e)) < eps, f"{label}: {a} != {e}"


def text_contents(node):
    return [c.get("content") for c in (node.get("children") or []) if c.get("type") == "Text"]


def body_run() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: hidden ───────────────────────────────────────────────
        assert_eq(tf.query("/external/visible"), False, "boot tooltip hidden")
        assert find_by_tag(paint(tf), POP) is None, "no overlay at boot"

        # ── hover shows the elevated rich surface ──────────────────────
        tf.hover(path=TRIGGER)
        assert_eq(tf.query("/external/visible"), True, "hover shows tooltip")
        assert_eq(tf.query("/external/hovered"), True, "hovered posture set")
        snap = paint(tf)
        pop = find_by_tag(snap, POP)
        assert pop is not None, "overlay present while shown"
        style = pop["style"]

        # surfaceContainer fill: opaque, distinct from the window Surface
        # AND from the trigger's SurfaceContainerHighest (a tonal tier up).
        pop_fill = rgba(style["fill"])
        assert pop_fill[3] == 255, f"rich surface opaque ({pop_fill})"
        surface_fill = rgba(snap["style"]["fill"])
        assert pop_fill[:3] != surface_fill[:3], "elevated surface tone != window Surface"
        trigger_fill = rgba(find_by_tag(snap, TRIGGER)["style"]["fill"])
        assert pop_fill[:3] != trigger_fill[:3], "surfaceContainer != trigger SurfaceContainerHighest"
        assert_eq(style["corner_radius"], 12, "rich tooltip corner radius")

        # elevated: the elevation(2) key + ambient shadow pair.
        shadows = style.get("shadows")
        assert isinstance(shadows, list) and len(shadows) == 2, "key + ambient shadow pair"
        key, amb = shadows
        _approx(key["blur"], KEY["blur"], "key blur")
        _approx(key["offset"]["y"], KEY["off_y"], "key offset.y")
        _approx(key["offset"]["x"], 0.0, "key offset.x")
        assert_eq(key["color"]["a"], KEY["alpha"], "key alpha")
        assert (key["color"]["r"], key["color"]["g"], key["color"]["b"]) == (0, 0, 0), "key black"
        _approx(amb["blur"], AMBIENT["blur"], "ambient blur")
        _approx(amb["offset"]["y"], AMBIENT["off_y"], "ambient offset.y")
        assert_eq(amb["color"]["a"], AMBIENT["alpha"], "ambient alpha")

        # title + supporting paragraph (single-sourced content).
        contents = text_contents(pop)
        assert_eq(contents, [TITLE, BODY], "title + supporting paragraph")

        # ── Escape dismisses while hover stays (WCAG 1.4.13) ───────────
        tf.key(path=TRIGGER, name="Escape")
        assert_eq(tf.query("/external/visible"), False, "Escape dismisses")
        assert_eq(tf.query("/external/dismissed"), True, "dismiss latch armed")
        assert_eq(tf.query("/external/hovered"), True, "still hovered after dismiss")
        assert find_by_tag(paint(tf), POP) is None, "overlay gone after dismiss"

        # ── leave + re-hover re-shows (latch clears on falling edge) ───
        tf.pointer_leave()
        assert_eq(tf.query("/external/hovered"), False, "leave clears hover")
        assert_eq(tf.query("/external/dismissed"), False, "leave clears the dismiss latch")
        tf.hover(path=TRIGGER)
        assert_eq(tf.query("/external/visible"), True, "re-hover re-shows after dismiss+leave")
        tf.pointer_leave()
        assert_eq(tf.query("/external/visible"), False, "leave hides again")

        # ── keyboard focus shows it; Escape dismisses ──────────────────
        focused = tf.request("focus/set", {"tag": TRIGGER}).result.get("focused")
        assert_eq(focused, TRIGGER, "trigger is the tab stop")
        assert_eq(tf.query("/external/focused"), True, "focus posture set")
        assert_eq(tf.query("/external/visible"), True, "focus shows tooltip (WCAG 1.4.13)")
        tf.key(path=TRIGGER, name="Escape")
        assert_eq(tf.query("/external/visible"), False, "Escape dismisses the focus-shown tooltip")

        # ── geometry for the pixel phase (trigger is boot-visible) ─────
        rects = abs_rects_of(paint(tf))
        assert TRIGGER in rects, "trigger has an absolute rect"
        tr = rects[TRIGGER]
        # Read the boot trigger tone from the paint scene (parity target).
        # At this point hover/focus are clear, so the trigger is at rest.
        trigger_rest = rgba(find_by_tag(paint(tf), TRIGGER)["style"]["fill"])

    # ── Phase 2 — live-pixel: the boot trigger chip ───────────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"
    tx, ty, tw, th = tr
    # Sample near the trigger's left edge, vertical centre — inside the
    # chip fill, clear of the centred label.
    spot = (tx + 8, ty + th // 2)
    px = sample_png_points(png, [spot])[0]
    assert_pixel_eq(px, (*trigger_rest[:3], 255), f"trigger chip tone {spot}", tolerance=12)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r729-")) / "tooltip_rich.png"
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
    sys.exit(run_demo("R729 Material 3 rich tooltip", body_run))
