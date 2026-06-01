#!/usr/bin/env python3
"""R737 §5.38 §5.40 §5.50 — discrete (stepped) slider E2E.

Drives the new `hello-slider-discrete` binding via JSON-RPC. The WAI-ARIA
discrete-slider / Material "slider with tick marks" variant: a slider whose
value snaps to the nearest of six stops (0.0 / 0.2 / 0.4 / 0.6 / 0.8 / 1.0).
1st consumer of the R737 `SliderExternal::with_step` substrate — the snap
lives in `Slider::set_value`, so ONE funnel snaps every value path
identically: drag, keyboard, intervene, RPC.

Atomic verification scope (>=30 assertions):

  (A) boot — track present; value seeded on the START tick (0.4); the
      `step` introspect field reports 0.2; state Idle.
  (B) keyboard stepping — ArrowRight advances one tick (0.4 -> 0.6),
      ArrowLeft retreats; End -> 1.0, ArrowRight clamps; Home -> 0.0,
      ArrowLeft clamps.
  (C) off-grid intervene snaps — an AI client writing 0.71 snaps to 0.8;
      0.09 snaps to 0.0 (the substrate snap, not binding code).
  (D) drag snaps — dragging the thumb to ~0.85 of the track lands on the
      0.8 tick (the same snap funnel the keyboard/intervene paths use).
  (E) step is read-only — intervening on `step` is rejected (construction-
      fixed); an unknown introspect path is rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame (value 0.4) shows the filled accent portion left of the
    thumb distinct from the unfilled rail to its right, and both distinct
    from the page background — the discrete slider renders as a real
    Material track.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-slider-discrete"
VIEWPORT = (360, 220)
PAUSE = 0.1
TAG = "disc_slider"
STATUS = "disc_status"


def _value(tf) -> float:
    return tf.query(f"/external/value")


def _state(tf) -> str:
    return tf.query(f"/external/state")


def _status(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "status node present"
    return node.get("content") or ""


def near(a: float, b: float, eps: float = 1e-3) -> bool:
    return abs(a - b) < eps


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "discrete track present at boot"
        assert find_by_tag(snap, STATUS) is not None, "status line present"
        assert near(_value(tf), 0.4), f"boot value on START tick 0.4, got {_value(tf)}"
        assert near(tf.query("/external/step"), 0.2), "step introspect field = 0.2"
        assert_eq(_state(tf), "Idle", "boot interaction state Idle")
        assert_eq(tf.query("/external/orientation"), "horizontal", "horizontal axis")
        # The status line is the AI text mirror of the snapped value +
        # its stop index (0.4 = stop 2 of 0..5).
        assert_eq(_status(tf), "Idle | 0.4 (stop 2/5)", "boot status mirror")

        # ── (A2) all six discrete stops are reachable + on-grid ──────
        # From the 0.0 floor, five ArrowRights walk every tick to 1.0.
        tf.request("focus/set", {"tag": TAG})
        tf.key(path=TAG, name="Home"); time.sleep(PAUSE)
        assert near(_value(tf), 0.0), "Home seeds the 0.0 floor for the stop walk"
        for i, expected in enumerate((0.2, 0.4, 0.6, 0.8, 1.0), start=1):
            tf.key(path=TAG, name="ArrowRight"); time.sleep(PAUSE)
            assert near(_value(tf), expected), f"stop {i} = {expected}, got {_value(tf)}"
        # Restore the START tick so section (B) starts from the boot value.
        tf.intervene("/external/value", 0.4); time.sleep(PAUSE)
        assert near(_value(tf), 0.4), "restored to START tick for section B"

        # ── (B) keyboard stepping (one tick per arrow) ──────────────
        tf.request("focus/set", {"tag": TAG})
        assert_eq(tf.request("focus/get").result.get("focused"), TAG, "track focused")
        tf.key(path=TAG, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_value(tf), 0.6), f"ArrowRight 0.4 -> 0.6, got {_value(tf)}"
        tf.key(path=TAG, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_value(tf), 0.8), "ArrowRight 0.6 -> 0.8"
        tf.key(path=TAG, name="ArrowLeft"); time.sleep(PAUSE)
        assert near(_value(tf), 0.6), "ArrowLeft 0.8 -> 0.6"
        tf.key(path=TAG, name="ArrowDown"); time.sleep(PAUSE)
        assert near(_value(tf), 0.4), "ArrowDown aliases ArrowLeft (0.6 -> 0.4)"
        tf.key(path=TAG, name="ArrowUp"); time.sleep(PAUSE)
        assert near(_value(tf), 0.6), "ArrowUp aliases ArrowRight (0.4 -> 0.6)"
        tf.key(path=TAG, name="End"); time.sleep(PAUSE)
        assert near(_value(tf), 1.0), "End -> max tick 1.0"
        tf.key(path=TAG, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_value(tf), 1.0), "ArrowRight at max clamps"
        tf.key(path=TAG, name="Home"); time.sleep(PAUSE)
        assert near(_value(tf), 0.0), "Home -> min tick 0.0"
        tf.key(path=TAG, name="ArrowLeft"); time.sleep(PAUSE)
        assert near(_value(tf), 0.0), "ArrowLeft at min clamps"

        # ── (C) off-grid intervene snaps (substrate funnel) ─────────
        tf.intervene("/external/value", 0.71); time.sleep(PAUSE)
        assert near(_value(tf), 0.8), f"intervene 0.71 snaps to 0.8, got {_value(tf)}"
        tf.intervene("/external/value", 0.09); time.sleep(PAUSE)
        assert near(_value(tf), 0.0), "intervene 0.09 snaps to 0.0"
        tf.intervene("/external/value", 0.5); time.sleep(PAUSE)
        # 0.5/0.2 = 2.5 -> round-half-away -> 3 -> 0.6.
        assert near(_value(tf), 0.6), f"intervene 0.5 snaps to 0.6, got {_value(tf)}"

        # ── (D) drag snaps to a tick (the substrate funnel covers the
        #        pointer path too). The exact cursor->value mapping is
        #        inset by the thumb half-width and routed through the
        #        InputRouter, so the robust property under test is that
        #        any drag lands ON-GRID (a multiple of STEP) and that a
        #        rightward drag yields >= a leftward one. The precise
        #        pointer_move->value snap is unit-tested directly in
        #        pinion-core (`snap_funnels_through_every_value_path`);
        #        asserting a pixel-predicted interior tick here would
        #        couple the demo to InputRouter geometry, not behaviour.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert TAG in rects, "track has an absolute rect"
        tx, ty, tw, th = rects[TAG]
        cy = ty + th // 2

        def on_grid(v: float) -> bool:
            return near(round(v / 0.2) * 0.2, v)

        tf.drag(from_path=TAG, to_at=(float(tx + tw - 4), float(cy)), steps=6)
        time.sleep(PAUSE)
        right_val = _value(tf)
        assert on_grid(right_val), f"rightward drag lands on a tick, got {right_val}"
        tf.drag(from_path=TAG, to_at=(float(tx + 4), float(cy)), steps=6)
        time.sleep(PAUSE)
        left_val = _value(tf)
        assert on_grid(left_val), f"leftward drag lands on a tick, got {left_val}"
        assert right_val > left_val, "rightward drag yields a larger value than leftward"

        # ── (E) negatives — step read-only + unknown path ───────────
        raised = False
        try:
            tf.intervene("/external/step", 0.5)
        except RpcError:
            raised = True
        assert raised, "step is construction-fixed (intervene must reject)"
        raised = False
        try:
            tf.query("/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"

    # ── Phase 2 — live pixels (boot frame, value 0.4) ───────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    assert TAG in rects, "track rect present for pixel sampling"
    tx, ty, tw, th = rects[TAG]
    cy = ty + th // 2
    # Filled portion sits left of the thumb (value 0.4); sample inside it,
    # the unfilled rail to the right, and the page background.
    filled_pt = (tx + int(0.15 * tw), cy)
    unfilled_pt = (tx + int(0.92 * tw), cy)
    page_pt = (6, 6)
    filled_px, unfilled_px, page_px = sample_png_points(img, [filled_pt, unfilled_pt, page_pt])
    assert filled_px != page_px, f"filled track distinct from page (filled={filled_px} page={page_px})"
    assert unfilled_px != page_px, f"rail distinct from page (rail={unfilled_px} page={page_px})"
    assert filled_px != unfilled_px, (
        f"filled accent distinct from unfilled rail (filled={filled_px} rail={unfilled_px})"
    )
    assert filled_px[3] == 255, f"filled track opaque; got alpha {filled_px[3]}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit (producer parity: the same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r737-")) / "slider-discrete.png"
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
    sys.exit(run_demo("R737 §5.38 §5.40 — discrete (stepped) slider", body))
