#!/usr/bin/env python3
"""R739 §5.38 §5.40 §5.50 — labeled-step slider (aria-valuetext) E2E.

Drives the new `hello-slider-labeled` binding via JSON-RPC. The WAI-ARIA
`aria-valuetext` named-stop slider: a discrete slider whose five stops carry
*names* — "Off / Low / Medium / High / Max" — instead of bare numbers
(0.0 / 0.25 / 0.5 / 0.75 / 1.0).

  * 2nd consumer of the R737 `SliderExternal::with_step` substrate — the
    snap lives in `Slider::set_value`, so ONE funnel snaps every value path
    identically: drag, keyboard, intervene, RPC.
  * 1st consumer of the R739 `AccessNode::value_text` axis — the snapped
    value reads a named label. The binding funnels the visible readout AND
    the a11y `aria-valuetext` through one `label_for`, so the painted label
    and the announced label can never diverge. This demo verifies the
    AI-observable side of that twin: the on-screen readout text (scene/
    snapshot) tracks the snapped value through every input path.

Atomic verification scope (>=30 assertions):

  (A) boot — track / readout / status present; value seeded on the START
      stop (0.5 = "Medium"); the `step` introspect field reports 0.25;
      state Idle; orientation horizontal; readout + status text mirror.
  (A2) every named stop reachable — Home seeds "Off", then four ArrowRights
      walk Low / Medium / High / Max, the readout naming each.
  (B) keyboard stepping — one stop per arrow, readout tracking the name;
      End -> Max clamps, Home -> Off clamps.
  (C) off-grid intervene snaps — 0.66 snaps to 0.75 ("High"); 0.1 snaps to
      0.0 ("Off") — the substrate snap, not binding code; readout follows.
  (D) drag snaps — dragging lands ON-GRID and the readout names the stop.
  (E) negatives — `step` is read-only (construction-fixed); an unknown
      introspect path is rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame (value 0.5) shows the filled accent portion left of the
    thumb distinct from the unfilled rail to its right, and both distinct
    from the page background — the labeled slider renders as a real Material
    track.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
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
    wait_until,
)

EXAMPLE = "hello-slider-labeled"
VIEWPORT = (360, 240)
TAG = "bright_slider"
READOUT = "bright_readout"
STATUS = "bright_status"


def _value(tf) -> float:
    return tf.query("/external/value")


def _state(tf) -> str:
    return tf.query("/external/state")


def _text_of(tf, tag: str) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} node present"
    return node.get("content") or ""


def _readout(tf) -> str:
    return _text_of(tf, READOUT)


def near(a: float, b: float, eps: float = 1e-3) -> bool:
    return abs(a - b) < eps


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "labeled track present at boot"
        assert find_by_tag(snap, READOUT) is not None, "readout present"
        assert find_by_tag(snap, STATUS) is not None, "status line present"
        assert near(_value(tf), 0.5), f"boot value on START stop 0.5, got {_value(tf)}"
        assert near(tf.query("/external/step"), 0.25), "step introspect field = 0.25"
        assert_eq(_state(tf), "Idle", "boot interaction state Idle")
        assert_eq(tf.query("/external/orientation"), "horizontal", "horizontal axis")
        # The readout is the visible twin of aria-valuetext; the status line
        # mirrors the snapped value + its stop index (0.5 = stop 2 of 0..4).
        assert_eq(_readout(tf), "Medium", "boot readout names the Medium stop")
        assert_eq(_text_of(tf, STATUS), "Idle | 0.50 (stop 2/4)", "boot status mirror")

        # ── (A2) every named stop is reachable + named ──────────────
        tf.request("focus/set", {"tag": TAG})
        tf.key(path=TAG, name="Home")
        wait_until(
            lambda: near(_value(tf), 0.0),
            desc="Home seeds the 0.0 (Off) floor",
        )
        assert_eq(_readout(tf), "Off", "0.0 stop is named Off")
        for expected_v, expected_label in (
            (0.25, "Low"), (0.5, "Medium"), (0.75, "High"), (1.0, "Max"),
        ):
            tf.key(path=TAG, name="ArrowRight")
            wait_until(
                lambda: near(_value(tf), expected_v),
                desc=f"stop {expected_v} reached",
            )
            assert_eq(_readout(tf), expected_label, f"{expected_v} named {expected_label}")
        # Restore the START stop for section (B).
        tf.intervene("/external/value", 0.5)
        wait_until(
            lambda: _readout(tf) == "Medium",
            desc="restored to Medium for section B",
        )

        # ── (B) keyboard stepping (one stop per arrow) ──────────────
        tf.request("focus/set", {"tag": TAG})
        assert_eq(tf.request("focus/get").result.get("focused"), TAG, "track focused")
        tf.key(path=TAG, name="ArrowRight")
        wait_until(
            lambda: near(_value(tf), 0.75) and _readout(tf) == "High",
            desc="ArrowRight Medium -> High",
        )
        tf.key(path=TAG, name="ArrowLeft")
        wait_until(
            lambda: near(_value(tf), 0.5) and _readout(tf) == "Medium",
            desc="ArrowLeft High -> Medium",
        )
        tf.key(path=TAG, name="ArrowDown")
        wait_until(
            lambda: near(_value(tf), 0.25) and _readout(tf) == "Low",
            desc="ArrowDown aliases ArrowLeft",
        )
        tf.key(path=TAG, name="ArrowUp")
        wait_until(
            lambda: near(_value(tf), 0.5) and _readout(tf) == "Medium",
            desc="ArrowUp aliases ArrowRight",
        )
        tf.key(path=TAG, name="End")
        wait_until(
            lambda: near(_value(tf), 1.0) and _readout(tf) == "Max",
            desc="End -> Max stop",
        )
        tf.key(path=TAG, name="ArrowRight")
        # Clamp no-op: plain read after the committed dispatch.
        assert near(_value(tf), 1.0) and _readout(tf) == "Max", "ArrowRight at Max clamps"
        tf.key(path=TAG, name="Home")
        wait_until(
            lambda: near(_value(tf), 0.0) and _readout(tf) == "Off",
            desc="Home -> Off stop",
        )
        tf.key(path=TAG, name="ArrowLeft")
        # Clamp no-op: plain read after the committed dispatch.
        assert near(_value(tf), 0.0) and _readout(tf) == "Off", "ArrowLeft at Off clamps"

        # ── (C) off-grid intervene snaps (substrate funnel) ─────────
        tf.intervene("/external/value", 0.66)
        wait_until(
            lambda: near(_value(tf), 0.75),
            desc="intervene 0.66 snaps to 0.75",
        )
        assert_eq(_readout(tf), "High", "snapped readout names High")
        tf.intervene("/external/value", 0.1)
        wait_until(
            lambda: near(_value(tf), 0.0),
            desc="intervene 0.1 snaps to 0.0",
        )
        assert_eq(_readout(tf), "Off", "snapped readout names Off")

        # ── (D) drag snaps to a stop (substrate funnel covers pointer).
        #        The exact cursor->value mapping is inset by the thumb
        #        half-width + routed through the InputRouter, so the robust
        #        property is on-grid + a named stop (the precise snap is
        #        unit-tested in pinion-core, not coupled to geometry here).
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert TAG in rects, "track has an absolute rect"
        tx, ty, tw, th = rects[TAG]
        cy = ty + th // 2

        def on_grid(v: float) -> bool:
            return near(round(v / 0.25) * 0.25, v)

        tf.drag(from_path=TAG, to_at=(float(tx + tw - 4), float(cy)), steps=6)
        wait_until(
            lambda: on_grid(_value(tf)) and _value(tf) > 0.5,
            desc="rightward drag lands on a high stop",
        )
        right_val = _value(tf)
        assert on_grid(right_val), f"rightward drag lands on a stop, got {right_val}"
        assert _readout(tf) in LABELS, f"readout names the dragged stop, got {_readout(tf)!r}"
        tf.drag(from_path=TAG, to_at=(float(tx + 4), float(cy)), steps=6)
        wait_until(
            lambda: on_grid(_value(tf)) and _value(tf) < right_val,
            desc="leftward drag lands on a lower stop",
        )
        left_val = _value(tf)
        assert on_grid(left_val), f"leftward drag lands on a stop, got {left_val}"
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

    # ── Phase 2 — live pixels (boot frame, value 0.5) ───────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    assert TAG in rects, "track rect present for pixel sampling"
    tx, ty, tw, th = rects[TAG]
    cy = ty + th // 2
    # Filled portion sits left of the thumb (value 0.5); sample inside it,
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


# The named-stop vocabulary (mirror of the binding's LABELS const).
LABELS = ("Off", "Low", "Medium", "High", "Max")


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render the boot frame to RGBA8 PNG via `PINION_SCREENSHOT`,
    bypassing winit (producer parity: the same `to_vello_cached`
    rasterizer the live window uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r739-")) / "slider-labeled.png"
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
    sys.exit(run_demo("R739 §5.38 §5.40 — labeled-step slider (aria-valuetext)", body))
