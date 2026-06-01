#!/usr/bin/env python3
"""R738 §5.38 §5.40 — dual-thumb range slider E2E.

Drives the new `hello-range-slider` binding via JSON-RPC. The WAI-ARIA
"Slider (Multi-Thumb)" pattern / Material range slider: two thumbs on one
shared track, the new `RangeSliderExternal` substrate coordinating them
(nearest-thumb pick on press + the monotonic `0 <= low <= high <= 1`
invariant — each setter clamps to the other thumb). 1st consumer of the
R738 substrate; the slider track/thumb M3 color contract is also lifted
this round to a 4-consumer SSOT in `pinion_widget_paint::slider`.

Atomic verification scope (>=30 assertions):

  (A) boot — track + both clickable/focusable thumb tags + status
      present; window seeded [0.25, 0.75]; state Idle; horizontal;
      active=high; status mirror.
  (B) keyboard, low thumb (Tab stop `range#low`) — ArrowRight raises it
      one KEY_STEP, ArrowLeft/Up/Down alias; the high thumb stays put;
      `End` on the low thumb clamps at the HIGH thumb (monotonic), never
      at 1.0; active follows the moved thumb.
  (C) keyboard, high thumb (Tab stop `range#high`) — symmetric: `Home`
      clamps at the LOW thumb, never at 0.0.
  (D) intervene monotonic clamp — writing the high thumb below the low
      clamps to the low value (and vice-versa); active is read-only;
      state read-only; unknown path rejected.
  (E) drag — grab a thumb (from_path) and drag to ~mid-track (NOT an
      extreme endpoint, which would saturate x_rel and mask the R738.1
      pick bug); the grabbed thumb moves toward the cursor, the other
      stays, `active` reports it, monotonic holds. This exercises the
      `capture_normalize_against_primary` opt-in (grabbing the low thumb
      drives the LOW thumb, not the far one). The exact cursor->value
      mapping is unit-tested in pinion-core.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame ([0.25, 0.75]) shows the accent fill *between* the
    thumbs distinct from the rail on either side and the page background,
    and a thumb knob (white centre + accent ring) distinct from the fill
    — the range slider renders as a real Material dual-thumb track with
    visible handles (the R738.1 visibility fix).
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

EXAMPLE = "hello-range-slider"
VIEWPORT = (400, 240)
PAUSE = 0.1
TAG = "range"
LOW = "range#low"
HIGH = "range#high"
STATUS = "range_status"


def _low(tf) -> float:
    return tf.query("/external/low")


def _high(tf) -> float:
    return tf.query("/external/high")


def _active(tf) -> str:
    return tf.query("/external/active")


def _state(tf) -> str:
    return tf.query("/external/state")


def _status(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "status node present"
    return node.get("content") or ""


def near(a: float, b: float, eps: float = 1e-3) -> bool:
    return abs(a - b) < eps


def set_window(tf, lo: float, hi: float) -> None:
    """Admin-reset the pair to `[lo, hi]` regardless of the current
    window. The monotonic clamp makes a naive two-write reset
    order-dependent (writing the low thumb above a collapsed high clamps
    it back), so widen the floor first (`low -> 0`), raise the ceiling,
    then drop the floor — a sequence that always lands the target."""
    tf.intervene("/external/low", 0.0)
    tf.intervene("/external/high", hi)
    tf.intervene("/external/low", lo)
    time.sleep(PAUSE)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "track present at boot"
        assert find_by_tag(snap, LOW) is not None, "low thumb present (clickable/focusable)"
        assert find_by_tag(snap, HIGH) is not None, "high thumb present"
        assert find_by_tag(snap, STATUS) is not None, "status line present"
        assert near(_low(tf), 0.25), f"boot low 0.25, got {_low(tf)}"
        assert near(_high(tf), 0.75), f"boot high 0.75, got {_high(tf)}"
        assert_eq(_state(tf), "Idle", "boot interaction state Idle")
        assert_eq(tf.query("/external/orientation"), "horizontal", "horizontal axis")
        assert_eq(_active(tf), "high", "boot active thumb = high")
        assert_eq(
            _status(tf),
            "Idle | min 0.25 max 0.75 (active: high)",
            "boot status mirror",
        )

        # ── (B) keyboard — the low thumb (Tab stop range#low) ───────
        tf.request("focus/set", {"tag": LOW})
        assert_eq(tf.request("focus/get").result.get("focused"), LOW, "low thumb focused")
        tf.key(path=LOW, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_low(tf), 0.30), f"ArrowRight low 0.25 -> 0.30, got {_low(tf)}"
        assert near(_high(tf), 0.75), "high thumb untouched by low-thumb arrow"
        assert_eq(_active(tf), "low", "active follows the moved (low) thumb")
        tf.key(path=LOW, name="ArrowLeft"); time.sleep(PAUSE)
        assert near(_low(tf), 0.25), "ArrowLeft low 0.30 -> 0.25"
        tf.key(path=LOW, name="ArrowUp"); time.sleep(PAUSE)
        assert near(_low(tf), 0.30), "ArrowUp aliases ArrowRight (low 0.25 -> 0.30)"
        tf.key(path=LOW, name="ArrowDown"); time.sleep(PAUSE)
        assert near(_low(tf), 0.25), "ArrowDown aliases ArrowLeft (low 0.30 -> 0.25)"
        # `End` on the low thumb clamps at the HIGH thumb, never at 1.0.
        tf.key(path=LOW, name="End"); time.sleep(PAUSE)
        assert near(_low(tf), 0.75), f"low End clamps at high (0.75), got {_low(tf)}"
        assert near(_high(tf), 0.75), "high thumb still 0.75"
        tf.key(path=LOW, name="Home"); time.sleep(PAUSE)
        assert near(_low(tf), 0.0), "low Home -> 0.0"
        tf.key(path=LOW, name="ArrowLeft"); time.sleep(PAUSE)
        assert near(_low(tf), 0.0), "low ArrowLeft at floor clamps"

        # ── (C) keyboard — the high thumb (Tab stop range#high) ─────
        # Reset to a known window first via the admin intervene channel.
        set_window(tf, 0.25, 0.75)
        tf.request("focus/set", {"tag": HIGH})
        assert_eq(tf.request("focus/get").result.get("focused"), HIGH, "high thumb focused")
        tf.key(path=HIGH, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_high(tf), 0.80), f"ArrowRight high 0.75 -> 0.80, got {_high(tf)}"
        assert near(_low(tf), 0.25), "low thumb untouched by high-thumb arrow"
        assert_eq(_active(tf), "high", "active follows the moved (high) thumb")
        tf.key(path=HIGH, name="End"); time.sleep(PAUSE)
        assert near(_high(tf), 1.0), "high End -> 1.0"
        tf.key(path=HIGH, name="ArrowRight"); time.sleep(PAUSE)
        assert near(_high(tf), 1.0), "high ArrowRight at ceiling clamps"
        # `Home` on the high thumb clamps at the LOW thumb, never at 0.0.
        tf.key(path=HIGH, name="Home"); time.sleep(PAUSE)
        assert near(_high(tf), 0.25), f"high Home clamps at low (0.25), got {_high(tf)}"
        assert near(_low(tf), 0.25), "low thumb still 0.25"

        # ── (D) intervene monotonic clamp + read-only fields ────────
        set_window(tf, 0.3, 0.7)
        # High write below the low thumb clamps to the low value.
        tf.intervene("/external/high", 0.1); time.sleep(PAUSE)
        assert near(_high(tf), 0.3), f"high clamps at low (0.3), got {_high(tf)}"
        # Low write above the high thumb clamps to the high value.
        tf.intervene("/external/low", 0.95); time.sleep(PAUSE)
        assert near(_low(tf), 0.3), f"low clamps at high (0.3), got {_low(tf)}"
        # `active` is derived (read-only); `state` is SCXML-owned.
        for ro_path, ro_val in (("/external/active", "low"), ("/external/state", "Dragging")):
            raised = False
            try:
                tf.intervene(ro_path, ro_val)
            except RpcError:
                raised = True
            assert raised, f"{ro_path} must reject intervene (read-only)"
        raised = False
        try:
            tf.query("/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"

        # ── (E) drag — invariants only (geometry is unit-tested) ────
        set_window(tf, 0.3, 0.7)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert TAG in rects, "track has an absolute rect"
        tx, ty, tw, th = rects[TAG]
        cy = ty + th // 2
        # Grab the LOW thumb (from_path presses its rect centre) and drag
        # to ~42% of the track — a realistic mid-track move, NOT an extreme
        # endpoint (which would saturate x_rel and mask the R738.1 pick
        # bug where grabbing low moved high). With the
        # capture_normalize_against_primary opt-in the cursor maps across
        # the track, so the LOW thumb (the one grabbed) moves toward the
        # cursor while the HIGH thumb stays and `active` reports `low`.
        tf.drag(from_path=LOW, to_at=(float(tx + int(0.42 * tw)), float(cy)), steps=8)
        time.sleep(PAUSE)
        assert_eq(_active(tf), "low", "grabbing the LOW thumb drives the LOW thumb")
        assert _low(tf) > 0.3 + 1e-3, f"low thumb moved RIGHT toward the cursor, got {_low(tf)}"
        assert near(_high(tf), 0.7), f"high thumb untouched by a low-thumb drag, got {_high(tf)}"
        assert _low(tf) <= _high(tf) + 1e-6, "monotonic low <= high after drag"
        # Grab the HIGH thumb and drag left to ~58%: the HIGH thumb moves
        # toward the cursor, the low thumb stays, active reports `high`.
        set_window(tf, 0.3, 0.7)
        tf.drag(from_path=HIGH, to_at=(float(tx + int(0.58 * tw)), float(cy)), steps=8)
        time.sleep(PAUSE)
        assert_eq(_active(tf), "high", "grabbing the HIGH thumb drives the HIGH thumb")
        assert _high(tf) < 0.7 - 1e-3, f"high thumb moved LEFT toward the cursor, got {_high(tf)}"
        assert near(_low(tf), 0.3), f"low thumb untouched by a high-thumb drag, got {_low(tf)}"
        assert _low(tf) <= _high(tf) + 1e-6, "monotonic low <= high after drag"

    # ── Phase 2 — live pixels (boot frame, [0.25, 0.75]) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    assert TAG in rects, "track rect present for pixel sampling"
    tx, ty, tw, th = rects[TAG]
    cy = ty + th // 2
    # The accent fill sits BETWEEN the thumbs (0.25..0.75 of the travel);
    # sample inside it, the rail to the left of the low thumb, the page
    # background, and a thumb body.
    between_pt = (tx + tw // 2, cy)
    rail_pt = (tx + int(0.06 * tw), cy)
    page_pt = (6, 6)
    between_px, rail_px, page_px = sample_png_points(img, [between_pt, rail_pt, page_pt])
    assert between_px != page_px, f"between-fill distinct from page (fill={between_px} page={page_px})"
    assert rail_px != page_px, f"rail distinct from page (rail={rail_px} page={page_px})"
    assert between_px != rail_px, (
        f"accent fill distinct from rail (fill={between_px} rail={rail_px})"
    )
    assert between_px[3] == 255, f"fill opaque; got alpha {between_px[3]}"
    # The low thumb knob (white centre + accent ring) must be distinct
    # from the accent fill it sits on — the R738.1 visibility fix (a bare
    # white knob was invisible on the light page). Sample the knob's
    # centre from its real paint rect.
    assert LOW in rects, "low thumb has a paint rect"
    lx, ly, lw, lh = rects[LOW]
    knob_px = sample_png_points(img, [(lx + lw // 2, ly + lh // 2)])[0]
    assert knob_px != between_px, f"low knob distinct from fill (knob={knob_px} fill={between_px})"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit (producer parity: the same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r738-")) / "range-slider.png"
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
    sys.exit(run_demo("R738 §5.38 §5.40 — dual-thumb range slider", body))
