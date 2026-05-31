#!/usr/bin/env python3
"""R728 §5.38 §5.40 single-select Material 3 segmented button.

`hello-segmented-button` is a pure composition over the R51.44
`RadioGroupExternal` coordinator (substrate-0): a segmented control is a
single-select radio group with a tonal-track skin. The selected segment
is an `Accent`-filled rounded pill carrying a leading check glyph; the
others are transparent (the `SurfaceContainerHighest` track shows
through). It boots with segment 0 ("Day") selected — M3 segmented
buttons always have a selected segment.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot selection == 0; the three segments resolve from one composite;
  * the selected segment paints an opaque Accent pill (a==255) + a
    leading check glyph (2 Text children vs 1 for the unselected);
  * `scene/click` on a segment switches the mutually-exclusive
    selection and moves the Accent pill + check glyph with it;
  * keyboard (focus the single tab stop, then ArrowRight/Left wrap,
    Home/End jump, d/w/m shortcuts) drives the same statechart path;
  * the `selected.<i>` introspect bits mirror the painted selection.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame, source of truth =
the paint scene's own fill colours, so the assertion is
introspection<->screen parity, not a hardcoded palette guess):
  * the selected pill renders the Accent the snapshot reports;
  * an unselected segment shows the track tone through its transparent
    fill;
  * the track strip renders SurfaceContainerHighest.

Run from the workspace root:
    cargo build -p hello-segmented-button --release
    python3 tools/demos/r728_segmented_button.py
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

EXAMPLE = "hello-segmented-button"
VIEWPORT = (420, 140)
TRACK = "view_mode"


def seg_tag(i: int) -> str:
    return f"view_mode#{i}"


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


def sel_index(tf):
    return tf.query("/external/selected_index")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: segment 0 selected, opaque Accent pill + check glyph ─
        assert_eq(sel_index(tf), 0, "boot default selection = Day (0)")
        snap = paint(tf)
        assert find_by_tag(snap, TRACK) is not None, "track tagged view_mode"
        for i in range(3):
            assert find_by_tag(snap, seg_tag(i)) is not None, f"segment {i} present"

        accent = seg_fill(snap, 0)
        assert accent[3] == 255, f"selected segment 0 is opaque ({accent})"
        assert seg_fill(snap, 1)[3] == 0, "segment 1 transparent (track shows)"
        assert seg_fill(snap, 2)[3] == 0, "segment 2 transparent (track shows)"
        assert_eq(text_child_count(snap, 0), 2, "selected segment has glyph + label")
        assert_eq(text_child_count(snap, 1), 1, "unselected segment has label only")

        assert_eq(tf.query("/external/selected.0"), True, "selected.0 bit set")
        assert_eq(tf.query("/external/selected.1"), False, "selected.1 bit clear")
        assert_eq(tf.query("/external/selected.2"), False, "selected.2 bit clear")

        # ── click segment 1: mutual exclusion moves pill + glyph ───────
        # scene/click leaves the cursor hovering the segment, so settle
        # back to Idle with pointer_leave (R719) before reading the pure
        # Accent fill (Hover would tint it via the state layer).
        tf.click(path=seg_tag(1))
        tf.pointer_leave()
        assert_eq(sel_index(tf), 1, "click selects segment 1")
        snap = paint(tf)
        sel1 = seg_fill(snap, 1)
        assert sel1[3] == 255, "segment 1 now opaque"
        assert sel1[:3] == accent[:3], "selected fill is the same Accent across segments"
        assert seg_fill(snap, 0)[3] == 0, "segment 0 back to transparent"
        assert_eq(tf.query("/external/selected.1"), True, "selected.1 bit set after click")
        assert_eq(tf.query("/external/selected.0"), False, "selected.0 cleared (mutual exclusion)")
        assert_eq(text_child_count(snap, 1), 2, "check glyph followed the selection to 1")
        assert_eq(text_child_count(snap, 0), 1, "glyph left segment 0")

        # ── click segment 2 ────────────────────────────────────────────
        tf.click(path=seg_tag(2))
        tf.pointer_leave()
        assert_eq(sel_index(tf), 2, "click selects segment 2")
        snap = paint(tf)
        assert seg_fill(snap, 2)[3] == 255, "segment 2 opaque"
        assert seg_fill(snap, 1)[3] == 0, "segment 1 transparent again"

        # ── keyboard: focus the single tab stop, then drive it ─────────
        focused = tf.request("focus/set", {"tag": TRACK}).result.get("focused")
        assert_eq(focused, TRACK, "group is the single tab stop")
        # From 2, ArrowRight wraps to 0.
        tf.key(path=TRACK, name="ArrowRight")
        assert_eq(sel_index(tf), 0, "ArrowRight wraps 2 -> 0")
        # ArrowLeft wraps 0 -> 2.
        tf.key(path=TRACK, name="ArrowLeft")
        assert_eq(sel_index(tf), 2, "ArrowLeft wraps 0 -> 2")
        tf.key(path=TRACK, name="Home")
        assert_eq(sel_index(tf), 0, "Home -> first segment")
        tf.key(path=TRACK, name="End")
        assert_eq(sel_index(tf), 2, "End -> last segment")
        tf.key(path=TRACK, name="w")
        assert_eq(sel_index(tf), 1, "shortcut w -> Week (1)")
        tf.key(path=TRACK, name="d")
        assert_eq(sel_index(tf), 0, "shortcut d -> Day (0)")
        tf.key(path=TRACK, name="m")
        assert_eq(sel_index(tf), 2, "shortcut m -> Month (2)")
        # Unrecognised key is swallowed without perturbing the selection.
        tf.key(path=TRACK, name="z")
        assert_eq(sel_index(tf), 2, "unrecognised key leaves selection intact")

        # Reset to the boot default so the geometry below matches the
        # screenshot's segment-0 selection.
        tf.key(path=TRACK, name="Home")
        assert_eq(sel_index(tf), 0, "reset selection to segment 0")

        # ── geometry + track tone for the pixel phase ──────────────────
        snap = paint(tf)
        rects = abs_rects_of(snap)
        assert TRACK in rects, "track has an absolute rect"
        for i in range(3):
            assert seg_tag(i) in rects, f"segment {i} has an absolute rect"
        track_fill = find_by_tag(snap, TRACK)["style"]["fill"]
        track_rgb = (track_fill["r"], track_fill["g"], track_fill["b"])
        surface_fill = snap["style"]["fill"]
        surface_rgb = (surface_fill["r"], surface_fill["g"], surface_fill["b"])

    # ── Phase 2 — live-pixel (boot frame: segment 0 selected) ─────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    sx, sy, _sw, sh = rects[seg_tag(0)]
    ux, uy, _uw, uh = rects[seg_tag(1)]
    tx, ty, tw, _th = rects[TRACK]
    mid0 = (sx + 10, sy + sh // 2)        # inside the Accent pill, clear of text
    mid1 = (ux + 10, uy + uh // 2)        # transparent segment -> track shows
    strip = (tx + tw // 2, ty + 2)        # track top padding strip
    corner = (5, 5)                       # outer window background (Surface)

    px0, px1, pstrip, pcorner = sample_png_points(png, [mid0, mid1, strip, corner])
    assert_pixel_eq(px0, (*accent[:3], 255), f"selected pill Accent {mid0}", tolerance=12)
    assert_pixel_eq(px1, (*track_rgb, 255), f"unselected shows track {mid1}", tolerance=12)
    assert_pixel_eq(pstrip, (*track_rgb, 255), f"track strip tone {strip}", tolerance=12)
    assert_pixel_eq(pcorner, (*surface_rgb, 255), f"window surface {corner}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r728-")) / "segmented.png"
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
    sys.exit(run_demo("R728 single-select segmented button", body))
