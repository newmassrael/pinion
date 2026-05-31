#!/usr/bin/env python3
"""R733 §5.38 §5.40 multi-select Material 3 segmented button.

`hello-segmented-multi` is a *toggle-button group*: any subset of the
three segments (Photos / Videos / Audio) may be pressed at once, and each
toggles independently. Unlike the single-select `hello-segmented-button`
(R728, a `RadioGroupExternal` with mutual exclusion), the interaction
substrate is N independent `ToggleExternal`s composed through
`create_extra_externals` (the `hello-accordion` cluster pattern) — zero new
coordinator. It boots with Photos + Videos on, Audio off (a multi-select
control has no "exactly one" invariant).

New reusable a11y primitive (R733): **`aria-pressed`**. Each segment lowers
to `role=button` carrying a toggled state — a `button` reflects
`aria-pressed`, distinct from the `aria-checked` a checkbox/switch/radio
reflects, even though both lower through the same AccessKit `set_toggled`.
The segments sit under a new WAI-ARIA `group` parent. The `aria-pressed` /
`group` mapping is unit-tested in the binding's `access_node`; this demo
verifies the underlying truth (each `ToggleExternal`'s `value`) + the paint
(pressed pill = opaque Accent, the same introspection<->screen parity the
R728 demo uses).

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: segments 0+1 pressed, 2 not; each resolves from its own external;
  * a pressed segment paints an opaque Accent pill (a==255) + a leading
    check glyph (2 Text children vs 1); unpressed = transparent (track);
  * `scene/click` on a segment toggles *only* that segment — pressing a
    3rd does NOT clear the others (multi-select independence, the key
    contrast with R728's mutual exclusion);
  * keyboard: each segment is its own Tab stop; Space/Enter toggle the
    focused segment, Arrow keys rove focus (wrapping), Home/End jump.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fill colours, so the assertion is introspection<->screen
parity, not a hardcoded palette guess):
  * a pressed pill renders the Accent the snapshot reports;
  * an unpressed segment shows the track tone through its transparent fill;
  * the track strip renders SurfaceContainerHighest; the window is Surface.

Run from the workspace root:
    cargo build -p hello-segmented-multi --release
    python3 tools/demos/r733_segmented_multi.py
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

EXAMPLE = "hello-segmented-multi"
VIEWPORT = (420, 160)
GROUP = "seg_multi_group"
N = 3


def seg_tag(i: int) -> str:
    return f"seg_multi_{i}"


def value(tf, i: int):
    """Pressed/On boolean of segment `i` via its own `ToggleExternal`."""
    return tf.query(f"/{seg_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def seg_fill(snap, i: int):
    """`(r, g, b, a)` of segment `i`'s box fill (a==0 == transparent)."""
    node = find_by_tag(snap, seg_tag(i))
    assert node is not None, f"segment {i} present"
    fill = node["style"]["fill"]
    assert fill is not None, f"segment {i} carries a box fill"
    return (fill["r"], fill["g"], fill["b"], fill.get("a", 255))


def text_child_count(snap, i: int) -> int:
    node = find_by_tag(snap, seg_tag(i))
    assert node is not None, f"segment {i} present"
    return sum(1 for c in (node.get("children") or []) if c.get("type") == "Text")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: Photos(0)+Videos(1) pressed, Audio(2) not ──────────────
        assert_eq(value(tf, 0), True, "boot: Photos (0) pressed")
        assert_eq(value(tf, 1), True, "boot: Videos (1) pressed")
        assert_eq(value(tf, 2), False, "boot: Audio (2) not pressed")

        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "track tagged seg_multi_group"
        for i in range(N):
            assert find_by_tag(snap, seg_tag(i)) is not None, f"segment {i} present"

        accent = seg_fill(snap, 0)
        assert accent[3] == 255, f"pressed segment 0 is opaque ({accent})"
        assert seg_fill(snap, 1)[3] == 255, "pressed segment 1 is opaque"
        assert seg_fill(snap, 2)[3] == 0, "unpressed segment 2 transparent (track shows)"
        assert seg_fill(snap, 1)[:3] == accent[:3], "all pressed pills share the Accent"
        assert_eq(text_child_count(snap, 0), 2, "pressed segment has glyph + label")
        assert_eq(text_child_count(snap, 2), 1, "unpressed segment has label only")

        # Capture boot geometry + track/surface tone now (the screenshot is
        # a fresh boot, so these match it regardless of later mutations).
        rects = abs_rects_of(snap)
        assert GROUP in rects, "track has an absolute rect"
        for i in range(N):
            assert seg_tag(i) in rects, f"segment {i} has an absolute rect"
        track_fill = find_by_tag(snap, GROUP)["style"]["fill"]
        track_rgb = (track_fill["r"], track_fill["g"], track_fill["b"])
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

        # ── click Audio(2): toggles ON, others UNTOUCHED (independence) ──
        tf.click(path=seg_tag(2))
        tf.pointer_leave()
        assert_eq(value(tf, 2), True, "click presses Audio (2)")
        assert_eq(value(tf, 0), True, "Photos stays pressed — NOT mutually exclusive")
        assert_eq(value(tf, 1), True, "Videos stays pressed — NOT mutually exclusive")
        assert seg_fill(paint(tf), 2)[3] == 255, "segment 2 now opaque Accent"

        # ── click Photos(0): toggles OFF, siblings UNTOUCHED ─────────────
        tf.click(path=seg_tag(0))
        tf.pointer_leave()
        assert_eq(value(tf, 0), False, "click un-presses Photos (0)")
        assert_eq(value(tf, 1), True, "Videos still pressed")
        assert_eq(value(tf, 2), True, "Audio still pressed")
        assert seg_fill(paint(tf), 0)[3] == 0, "segment 0 back to transparent"

        # ── keyboard: each segment is its own Tab stop ───────────────────
        focused = tf.request("focus/set", {"tag": seg_tag(0)}).result.get("focused")
        assert_eq(focused, seg_tag(0), "segment 0 is an independent Tab stop")

        # Space toggles ONLY the focused segment (0: off -> on).
        tf.key(path=seg_tag(0), name="Space")
        assert_eq(value(tf, 0), True, "Space toggles focused segment 0 on")
        assert_eq(value(tf, 1), True, "sibling 1 untouched by Space")

        # Enter also toggles (0: on -> off).
        tf.key(path=seg_tag(0), name="Enter")
        assert_eq(value(tf, 0), False, "Enter toggles focused segment 0 off")

        # Arrow keys rove FOCUS (not selection — the multi-select contrast).
        tf.key(path=seg_tag(0), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(1),
                  "ArrowRight: focus 0 -> 1")
        tf.key(path=seg_tag(1), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(2),
                  "ArrowRight: focus 1 -> 2")
        tf.key(path=seg_tag(2), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(0),
                  "ArrowRight wraps: focus 2 -> 0")
        tf.key(path=seg_tag(0), name="ArrowLeft")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(2),
                  "ArrowLeft wraps: focus 0 -> 2")
        tf.key(path=seg_tag(2), name="Home")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(0),
                  "Home -> first segment")
        tf.key(path=seg_tag(0), name="End")
        assert_eq(tf.request("focus/get").result.get("focused"), seg_tag(2),
                  "End -> last segment")

        # Space on the now-focused last segment toggles it (2: on -> off).
        tf.key(path=seg_tag(2), name="Space")
        assert_eq(value(tf, 2), False, "Space toggles focused last segment off")

    # ── Phase 2 — live-pixel (boot frame: segments 0+1 pressed) ──────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    sx, sy, _sw, sh = rects[seg_tag(0)]        # pressed (Photos)
    ux, uy, _uw, uh = rects[seg_tag(2)]        # unpressed (Audio)
    tx, ty, tw, _th = rects[GROUP]
    mid_pressed = (sx + 10, sy + sh // 2)      # inside the Accent pill, clear of text
    mid_unpressed = (ux + 10, uy + uh // 2)    # transparent segment -> track shows
    strip = (tx + tw // 2, ty + 2)             # track top padding strip
    corner = (5, 5)                            # outer window background (Surface)

    p_pressed, p_unpressed, p_strip, p_corner = sample_png_points(
        png, [mid_pressed, mid_unpressed, strip, corner])
    assert_pixel_eq(p_pressed, (*accent[:3], 255), f"pressed pill Accent {mid_pressed}", tolerance=12)
    assert_pixel_eq(p_unpressed, (*track_rgb, 255), f"unpressed shows track {mid_unpressed}", tolerance=12)
    assert_pixel_eq(p_strip, (*track_rgb, 255), f"track strip tone {strip}", tolerance=12)
    assert_pixel_eq(p_corner, (*surface_rgb, 255), f"window surface {corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r733-")) / "segmented-multi.png"
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
    sys.exit(run_demo("R733 multi-select segmented button", body))
