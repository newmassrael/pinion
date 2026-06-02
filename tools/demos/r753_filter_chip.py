#!/usr/bin/env python3
"""R753 §5.38 §5.40 Material 3 filter chips (`hello-filter-chip`).

A free-standing bar of four independently-toggleable filter chips
(Nearby / Open now / Top rated / Offers). This is the **2nd consumer** of
the `pinion_core::widgets::toggle_group` interaction substrate — the same
N-independent-`ToggleExternal`s-under-a-WAI-ARIA-`group` model the
multi-select segmented button (`hello-segmented-multi`, R733) uses. Building
this binding is what forced that lift: the keyboard model, the introspect
reader, the boot seed and the AccessKit `group` + `button[aria-pressed]`
tree are now shared verbatim, and only the *paint* diverges.

What diverges is the chip skin: detached 8 px-corner rounded rectangles
(not a joined stadium track) with an **`Outline` border while unselected**
that drops away when selected (the defining M3 filter-chip affordance). The
selected fill is an opaque tonal container; the hover/pressed/disabled
overlay routes through the shared R752 `state_layer` SSOT.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: chips 0+2 selected, 1+3 not; each resolves from its own external;
  * a selected chip paints an opaque fill, drops its `Outline` border, and
    shows a leading check glyph (2 Text children vs 1); an unselected chip
    is transparent (a==0) and carries a non-null `style.border`;
  * `scene/click` on a chip toggles *only* that chip — pressing another
    does NOT clear the others (multi-select independence);
  * keyboard: each chip is its own Tab stop; Space/Enter toggle the focused
    chip, Arrow keys rove focus (wrapping), Home/End jump.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own fill colours, so the assertion is introspection<->screen
parity, not a hardcoded palette guess):
  * a selected chip renders the opaque fill the snapshot reports;
  * an unselected chip is transparent, so the window Surface shows through;
  * the window background is Surface.

Run from the workspace root:
    cargo build -p hello-filter-chip --release
    python3 tools/demos/r753_filter_chip.py
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

EXAMPLE = "hello-filter-chip"
VIEWPORT = (480, 120)
GROUP = "chip_group"
N = 4


def chip_tag(i: int) -> str:
    return f"chip_{i}"


def value(tf, i: int):
    """Selected/On boolean of chip `i` via its own `ToggleExternal`."""
    return tf.query(f"/{chip_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def chip_node(snap, i: int):
    node = find_by_tag(snap, chip_tag(i))
    assert node is not None, f"chip {i} present"
    return node


def chip_fill(snap, i: int):
    """`(r, g, b, a)` of chip `i`'s box fill (a==0 == transparent)."""
    fill = chip_node(snap, i)["style"]["fill"]
    assert fill is not None, f"chip {i} carries a box fill"
    return (fill["r"], fill["g"], fill["b"], fill.get("a", 255))


def chip_border(snap, i: int):
    """The chip's `style.border` (None when absent)."""
    return chip_node(snap, i)["style"].get("border")


def text_child_count(snap, i: int) -> int:
    node = chip_node(snap, i)
    return sum(1 for c in (node.get("children") or []) if c.get("type") == "Text")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: Nearby(0)+Top rated(2) selected, others not ────────────
        assert_eq(value(tf, 0), True, "boot: Nearby (0) selected")
        assert_eq(value(tf, 1), False, "boot: Open now (1) not selected")
        assert_eq(value(tf, 2), True, "boot: Top rated (2) selected")
        assert_eq(value(tf, 3), False, "boot: Offers (3) not selected")

        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "chip row tagged chip_group"
        for i in range(N):
            assert find_by_tag(snap, chip_tag(i)) is not None, f"chip {i} present"

        # Selected chip: opaque fill, NO border, check glyph + label.
        sel = chip_fill(snap, 0)
        assert sel[3] == 255, f"selected chip 0 is opaque ({sel})"
        assert chip_border(snap, 0) is None, "selected chip drops the Outline border"
        assert_eq(text_child_count(snap, 0), 2, "selected chip has check glyph + label")
        assert chip_fill(snap, 2)[3] == 255, "selected chip 2 is opaque"
        assert chip_fill(snap, 2)[:3] == sel[:3], "selected chips share the fill tone"
        assert chip_border(snap, 2) is None, "selected chip 2 has no border"

        # Unselected chip: transparent fill, an Outline border, label only.
        assert chip_fill(snap, 1)[3] == 0, "unselected chip 1 transparent (surface shows)"
        b1 = chip_border(snap, 1)
        assert b1 is not None, "unselected chip 1 carries an Outline border"
        assert b1["width"] >= 1, "outline has a positive width"
        assert_eq(text_child_count(snap, 1), 1, "unselected chip has label only")
        assert chip_fill(snap, 3)[3] == 0, "unselected chip 3 transparent"
        assert chip_border(snap, 3) is not None, "unselected chip 3 is outlined"

        # Capture boot geometry + surface tone now (the screenshot is a
        # fresh boot, so these match it regardless of later mutations).
        rects = abs_rects_of(snap)
        assert GROUP in rects, "chip row has an absolute rect"
        for i in range(N):
            assert chip_tag(i) in rects, f"chip {i} has an absolute rect"
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

        # ── click Open now(1): toggles ON, others UNTOUCHED (independence) ─
        tf.click(path=chip_tag(1))
        tf.pointer_leave()
        assert_eq(value(tf, 1), True, "click selects Open now (1)")
        assert_eq(value(tf, 0), True, "Nearby stays selected — NOT exclusive")
        assert_eq(value(tf, 2), True, "Top rated stays selected — NOT exclusive")
        assert_eq(value(tf, 3), False, "Offers stays unselected")
        snap = paint(tf)
        assert chip_fill(snap, 1)[3] == 255, "chip 1 now opaque"
        assert chip_border(snap, 1) is None, "chip 1 dropped its border on select"

        # ── click Nearby(0): toggles OFF, siblings UNTOUCHED ─────────────
        tf.click(path=chip_tag(0))
        tf.pointer_leave()
        assert_eq(value(tf, 0), False, "click un-selects Nearby (0)")
        assert_eq(value(tf, 1), True, "Open now still selected")
        assert_eq(value(tf, 2), True, "Top rated still selected")
        snap = paint(tf)
        assert chip_fill(snap, 0)[3] == 0, "chip 0 back to transparent"
        assert chip_border(snap, 0) is not None, "chip 0 regained its Outline border"

        # ── keyboard: each chip is its own Tab stop ──────────────────────
        focused = tf.request("focus/set", {"tag": chip_tag(0)}).result.get("focused")
        assert_eq(focused, chip_tag(0), "chip 0 is an independent Tab stop")

        # Space toggles ONLY the focused chip (0: off -> on).
        tf.key(path=chip_tag(0), name="Space")
        assert_eq(value(tf, 0), True, "Space selects focused chip 0")
        assert_eq(value(tf, 1), True, "sibling 1 untouched by Space")

        # Enter also toggles (0: on -> off).
        tf.key(path=chip_tag(0), name="Enter")
        assert_eq(value(tf, 0), False, "Enter un-selects focused chip 0")

        # Arrow keys rove FOCUS (not selection — the multi-select contrast).
        tf.key(path=chip_tag(0), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(1),
                  "ArrowRight: focus 0 -> 1")
        tf.key(path=chip_tag(1), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(2),
                  "ArrowRight: focus 1 -> 2")
        tf.key(path=chip_tag(2), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(3),
                  "ArrowRight: focus 2 -> 3")
        tf.key(path=chip_tag(3), name="ArrowRight")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(0),
                  "ArrowRight wraps: focus 3 -> 0")
        tf.key(path=chip_tag(0), name="ArrowLeft")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(3),
                  "ArrowLeft wraps: focus 0 -> 3")
        tf.key(path=chip_tag(3), name="Home")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(0),
                  "Home -> first chip")
        tf.key(path=chip_tag(0), name="End")
        assert_eq(tf.request("focus/get").result.get("focused"), chip_tag(3),
                  "End -> last chip")

        # Space on the now-focused last chip selects it (3: off -> on).
        tf.key(path=chip_tag(3), name="Space")
        assert_eq(value(tf, 3), True, "Space selects focused last chip")

    # ── Phase 2 — live-pixel (boot frame: chips 0+2 selected) ────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    sx, sy, _sw, sh = rects[chip_tag(0)]       # selected (Nearby)
    ux, uy, _uw, uh = rects[chip_tag(1)]       # unselected (Open now)
    mid_selected = (sx + 14, sy + sh // 2)     # inside the opaque fill, clear of text
    mid_unselected = (ux + 14, uy + uh // 2)   # transparent chip -> window surface
    corner = (5, 5)                            # outer window background (Surface)

    p_selected, p_unselected, p_corner = sample_png_points(
        png, [mid_selected, mid_unselected, corner])
    assert_pixel_eq(p_selected, (*sel[:3], 255), f"selected chip fill {mid_selected}", tolerance=12)
    assert_pixel_eq(p_unselected, (*surface_rgb, 255), f"unselected shows surface {mid_unselected}", tolerance=12)
    assert_pixel_eq(p_corner, (*surface_rgb, 255), f"window surface {corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r753-")) / "filter-chip.png"
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
    sys.exit(run_demo("R753 Material 3 filter chips", body))
