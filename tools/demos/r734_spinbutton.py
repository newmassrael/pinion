#!/usr/bin/env python3
"""R734 §5.38 §5.40 WAI-ARIA spinbutton (bounded stepped numeric field).

`hello-spinbutton` is a `[ - ][ value ][ + ]` numeric field bounded to
[0, 10], single step 1, page step 5, booting at 3. The interaction
substrate is the R734 `SpinButtonExternal` — a plain value holder (no
interaction statechart, mirroring `ProgressBarExternal`), distinct from
`SliderExternal` (continuous normalized drag): a spin button holds a value
in domain units with an explicit step, stepped discretely.

The - / + affordances are paint regions tagged `spin#dec` / `spin#inc`, so
a click routes through the R51.42 composite hit-target protocol to
`invoke("send", "dec:PointerUp")` / `"inc:PointerUp"` against the single
external. The numeric readout is the focusable `spinbutton` (one Tab stop).

New a11y primitive (R734): `AriaRole::SpinButton` carrying the value as
`AccessValue::Float` (aria-valuenow/min/max) — the 3rd Float consumer
after Slider + ProgressBar, on the operable role (Focus + Increment +
Decrement actions). The role/value lowering is unit-tested in the binding;
this demo verifies the underlying value (the SpinButtonExternal) across
every input channel + the live paint.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot value 3, range [0, 10], step 1;
  * pointer: click `spin#inc` / `spin#dec` step the value by 1 and clamp;
  * keyboard (focus the spinbutton): Arrow +/- 1, Page +/- 5, Home/End to
    min/max;
  * AI actions: invoke increment/decrement/page_up/page_down + intervene
    value all reach the identical clamped arithmetic.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fills): the field + stepper regions render the
SurfaceContainerHighest tone; the window is Surface.

Run from the workspace root:
    cargo build -p hello-spinbutton --release
    python3 tools/demos/r734_spinbutton.py
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

EXAMPLE = "hello-spinbutton"
VIEWPORT = (320, 160)
TAG = "spin"
DEC = "spin#dec"
INC = "spin#inc"


def val(tf):
    return tf.query("/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: value 3, range [0, 10], step 1 ────────────────────────
        assert_eq(val(tf), 3.0, "boot value = 3")
        assert_eq(tf.query("/external/min"), 0.0, "min = 0")
        assert_eq(tf.query("/external/max"), 10.0, "max = 10")
        assert_eq(tf.query("/external/step"), 1.0, "step = 1")

        snap = paint(tf)
        assert find_by_tag(snap, TAG) is not None, "field tagged spin (paint root)"
        assert find_by_tag(snap, DEC) is not None, "decrement region present"
        assert find_by_tag(snap, INC) is not None, "increment region present"

        # ── pointer: click + / - steps by 1 and clamps ──────────────────
        tf.click(path=INC)
        tf.pointer_leave()
        assert_eq(val(tf), 4.0, "click + : 3 -> 4")
        tf.click(path=INC)
        tf.pointer_leave()
        assert_eq(val(tf), 5.0, "click + : 4 -> 5")
        tf.click(path=DEC)
        tf.pointer_leave()
        assert_eq(val(tf), 4.0, "click - : 5 -> 4")

        # ── keyboard: focus the spinbutton (single Tab stop) ─────────────
        focused = tf.request("focus/set", {"tag": TAG}).result.get("focused")
        assert_eq(focused, TAG, "spinbutton is the Tab stop")

        tf.key(path=TAG, name="ArrowUp")
        assert_eq(val(tf), 5.0, "ArrowUp: +1")
        tf.key(path=TAG, name="ArrowRight")
        assert_eq(val(tf), 6.0, "ArrowRight: +1 (alias)")
        tf.key(path=TAG, name="ArrowDown")
        assert_eq(val(tf), 5.0, "ArrowDown: -1")
        tf.key(path=TAG, name="ArrowLeft")
        assert_eq(val(tf), 4.0, "ArrowLeft: -1 (alias)")
        tf.key(path=TAG, name="PageUp")
        assert_eq(val(tf), 9.0, "PageUp: +5")
        tf.key(path=TAG, name="PageDown")
        assert_eq(val(tf), 4.0, "PageDown: -5")
        tf.key(path=TAG, name="End")
        assert_eq(val(tf), 10.0, "End: -> max")
        tf.key(path=TAG, name="Home")
        assert_eq(val(tf), 0.0, "Home: -> min")

        # ── clamping at the bounds (no over/underflow) ───────────────────
        tf.key(path=TAG, name="ArrowDown")
        assert_eq(val(tf), 0.0, "ArrowDown at min clamps (stays 0)")
        tf.key(path=TAG, name="End")
        tf.key(path=TAG, name="PageUp")
        assert_eq(val(tf), 10.0, "PageUp at max clamps (stays 10)")

        # ── AI actions: invoke + intervene reach identical arithmetic ────
        assert_eq(tf.invoke("/external/decrement", None), 9.0, "invoke decrement: 10 -> 9")
        assert_eq(tf.invoke("/external/page_down", None), 4.0, "invoke page_down: 9 -> 4")
        assert_eq(tf.invoke("/external/increment", None), 5.0, "invoke increment: 4 -> 5")
        assert_eq(tf.invoke("/external/page_up", None), 10.0, "invoke page_up: 5 +5 -> 10")
        tf.intervene("/external/value", 7)
        assert_eq(val(tf), 7.0, "intervene value 7")
        tf.intervene("/external/value", 999)
        assert_eq(val(tf), 10.0, "intervene clamps above max")
        tf.intervene("/external/value", -5)
        assert_eq(val(tf), 0.0, "intervene clamps below min")

        # ── geometry + field tone for the pixel phase ───────────────────
        snap = paint(tf)
        rects = abs_rects_of(snap)
        assert TAG in rects, "field has an absolute rect"
        assert DEC in rects and INC in rects, "both stepper regions have rects"
        field_fill = find_by_tag(snap, TAG)["style"]["fill"]
        field_rgb = (field_fill["r"], field_fill["g"], field_fill["b"])
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

    # ── Phase 2 — live-pixel (boot frame) ────────────────────────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    dx, dy, _dw, dh = rects[DEC]
    tx, ty, tw, _th = rects[TAG]
    dec_mid = (dx + 8, dy + dh // 2)       # inside the decrement region (clear of glyph)
    field_strip = (tx + tw // 2, ty + 2)   # field top edge strip
    corner = (4, 4)                        # outer window background (Surface)

    p_dec, p_field, p_corner = sample_png_points(png, [dec_mid, field_strip, corner])
    assert_pixel_eq(p_dec, (*field_rgb, 255), f"stepper region tone {dec_mid}", tolerance=12)
    assert_pixel_eq(p_field, (*field_rgb, 255), f"field strip tone {field_strip}", tolerance=12)
    assert_pixel_eq(p_corner, (*surface_rgb, 255), f"window surface {corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r734-")) / "spinbutton.png"
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
    sys.exit(run_demo("R734 WAI-ARIA spinbutton", body))
