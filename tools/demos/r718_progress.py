#!/usr/bin/env python3
"""R718 §5.38 §5.40 §5.50 — determinate linear ProgressBar E2E.

Drives the new `hello-progress` binding via JSON-RPC. The WAI-ARIA 1.2
§3.5 `progressbar` is a descriptive (non-interactive) widget: a normalized
fraction reported as `aria-valuenow` on a passive `AriaRole::ProgressBar`
node. Its single observable axis is the fraction, **writable through the
§5.15 introspect channel** — `intervene("/external/value", Float)`, the
same side door the RPC route and a real application's progress updater
both use (mirroring `SliderExternal`'s value channel, minus the pointer).

`hello-progress` is the 2nd `AccessValue::Float` consumer after the
slider (anticipated in `slider.rs`), built on a plain `External` value
holder (the `TooltipExternal` descriptive-widget precedent — no SCXML).

Atomic verification scope (>=30 assertions):

  (A) boot shape — track + percent readout present; `/external/value`
      reports the boot fraction (0.40); the normalized range bounds
      (`min`=0 / `max`=1) read back; the status mirror reads "40%".
  (B) set value via intervene — 0.75 / 0.0 / 1.0 / 0.25 each update both
      `/external/value` and the percent readout in lockstep.
  (C) clamp — out-of-range writes (-0.5, 1.5) saturate to [0, 1].
  (D) int coercion — an integer payload (1 / 0) is accepted (an AI
      client may send a bare int for a full / empty bar).
  (E) negatives — the fixed range bounds reject intervene (read-only);
      a wrong-typed value and unknown paths are rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame paints the bar at 40 %: the leading 40 % is the accent
    active indicator, the trailing 60 % the inactive-track tone, against
    the page surface. Three distinct, opaque regions — the determinate
    fill is visible in real pixels (the R706 [[introspection-from-paint-
    not-screen]] guard: structure alone can't prove the fill rasterized).
"""

from __future__ import annotations

import math
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
# Match the binding's Fixed initial size so the paint snapshot's computed
# rects line up with the PINION_SCREENSHOT PNG pixels.
VIEWPORT = (360, 160)

TRACK = "progress"
STATUS = "progress_status"
TRACK_W = 240


def _value(tf) -> float:
    return float(tf.query("/external/value"))


def _status(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "percent readout present"
    return node.get("content") or ""


def _approx(actual: float, expected: float, label: str) -> None:
    assert math.isclose(actual, expected, abs_tol=1e-4), (
        f"{label}: got {actual}, want ~{expected}"
    )


def _set(tf, value) -> None:
    tf.intervene("/external/value", value)


def _expect_rpc_error(fn, label: str) -> None:
    raised = False
    try:
        fn()
    except RpcError:
        raised = True
    assert raised, f"{label} must be rejected"


def body() -> None:
    with RpcSubprocess("hello-progress", boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TRACK) is not None, "track present at boot"
        assert find_by_tag(snap, STATUS) is not None, "percent readout present"
        _approx(_value(tf), 0.40, "boot value")
        _approx(float(tf.query("/external/min")), 0.0, "min bound")
        _approx(float(tf.query("/external/max")), 1.0, "max bound")
        assert_eq(_status(tf), "40%", "boot percent readout")

        # ── (B) set value via intervene → value + readout lockstep ──
        for frac, pct in [(0.75, "75%"), (0.0, "0%"), (1.0, "100%"), (0.25, "25%")]:
            _set(tf, frac)
            _approx(_value(tf), frac, f"value after set {frac}")
            assert_eq(_status(tf), pct, f"readout after set {frac}")

        # ── (C) clamp out-of-range writes ───────────────────────────
        _set(tf, -0.5)
        _approx(_value(tf), 0.0, "negative clamps to 0")
        assert_eq(_status(tf), "0%", "negative readout clamps to 0%")
        _set(tf, 1.5)
        _approx(_value(tf), 1.0, "overshoot clamps to 1")
        assert_eq(_status(tf), "100%", "overshoot readout clamps to 100%")

        # ── (D) integer-payload coercion ────────────────────────────
        _set(tf, 0)  # JSON int → Int arm → 0.0
        _approx(_value(tf), 0.0, "int 0 coerces to empty")
        _set(tf, 1)  # JSON int → Int arm → 1.0
        _approx(_value(tf), 1.0, "int 1 coerces to full")

        # ── (E) negatives ───────────────────────────────────────────
        _expect_rpc_error(
            lambda: tf.intervene("/external/min", 0.2), "min bound read-only"
        )
        _expect_rpc_error(
            lambda: tf.intervene("/external/max", 0.8), "max bound read-only"
        )
        _expect_rpc_error(
            lambda: tf.intervene("/external/value", True), "bool value type-mismatch"
        )
        _expect_rpc_error(
            lambda: tf.intervene("/external/speed", 1.0), "unknown intervene path"
        )
        _expect_rpc_error(lambda: tf.query("/external/nope"), "unknown query path")

        # restore a mid value so the live state is sane on shutdown
        _set(tf, 0.6)
        _approx(_value(tf), 0.6, "restored mid value")
        assert_eq(_status(tf), "60%", "restored readout")

    # ── Phase 2 — live pixels (fresh boot frame at 40 %) ────────────
    snap, rects = _boot_snapshot_and_rects()
    assert TRACK in rects, "track has a computed rect"
    tx, ty, tw, th = rects[TRACK]
    assert abs(tw - TRACK_W) <= 2, f"track is ~{TRACK_W}px wide; got {tw}"
    mid_y = ty + th // 2
    # 40 % filled: sample well inside the filled (20 %) and unfilled (80 %)
    # spans so anti-aliasing at the fill boundary can't taint the read.
    filled_pt = (tx + int(0.20 * tw), mid_y)
    unfilled_pt = (tx + int(0.80 * tw), mid_y)
    page_pt = (6, 6)

    img = read_png_rgba8(capture_screenshot())
    filled_px, unfilled_px, page_px = sample_png_points(
        img, [filled_pt, unfilled_pt, page_pt]
    )
    assert filled_px != page_px, (
        f"active indicator is distinct from the page (filled={filled_px} page={page_px})"
    )
    assert unfilled_px != page_px, (
        f"inactive track is distinct from the page (unfilled={unfilled_px} page={page_px})"
    )
    assert filled_px != unfilled_px, (
        f"active indicator vs inactive track distinct "
        f"(filled={filled_px} unfilled={unfilled_px})"
    )
    assert filled_px[3] == 255, f"active indicator opaque; got alpha {filled_px[3]}"
    assert sum(filled_px[:3]) > 0, f"active indicator not pure black; got {filled_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess("hello-progress", boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render hello-progress's boot frame (bar at 40 %) to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: the same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r718-")) / "progress.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-progress"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-progress", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R718 §5.38 §5.40 — determinate ProgressBar", body))
