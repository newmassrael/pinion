#!/usr/bin/env python3
"""R1429 §5.35 §5.15 — a raw pointer surface reacts to TILT.

R1423 added the pen FORCE axis; R1429 adds the pen LEAN. `External::pointer_tilt`
forwards the W3C `PointerEvent.tiltX` / `tiltY` (the toolkit `xTilt/yTilt`),
in degrees, each axis -90..=90, alongside each `pointer_move` and on a standalone
change, so a tilt-aware surface reads the live angle.

winit 0.30 exposes no tablet-tilt axis, so the sole driver is `scene/pointer_tilt`
(§2 #2, the AI-first primary path) — positionless, out-of-band like
`scene/pointer_pressure`, delivered to the surface under the pointer at once. A
tilt-reactive surface is fully exercisable headless, no tablet required.

hello-raw-pointer paints a pen-tip MARKER that leans off the live cursor in the
tilt direction: `offset = (tilt / 90) * 40` px along each axis (positive tilt_x
leans right, positive tilt_y leans down — the W3C sign). This demo drives the
tilt over the wire and reads BOTH the `tilt_x` / `tilt_y` introspect fields AND
the marker's computed rect (the tip leans with the angle, returns to the cursor
at zero, follows the cursor, vanishes off the pane).

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1429_tilt.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    RpcError,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (720, 440)
PANE = "pane"
TIP = "tilt.tip"
EXT = "/external"

# Mirror of the binding's PANE_RECT = Rect::new(16, 48, WIN_W - 32, WIN_H - 104)
# and TILT_SPAN_PX = 40.0 (max pixel offset at full ±90° lean).
RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104
TILT_SPAN = 40.0


def pane_at(fx: float, fy: float) -> tuple[float, float]:
    """The window pixel at fraction `(fx, fy)` across the pane rect."""
    return (RECT_X + fx * RECT_W, RECT_Y + fy * RECT_H)


CENTER = pane_at(0.5, 0.5)
RIGHT = pane_at(0.8, 0.5)
ABOVE = (360.0, 12.0)  # off the pane (above it)


def tip_center(snap):
    """The pen-tip marker's centre `(x, y)`, or `None` when the marker is absent."""
    node = find_by_tag(snap, TIP)
    if node is None:
        return None
    r = node["rect"]
    return (r["x"] + r["w"] / 2.0, r["y"] + r["h"] / 2.0)


def assert_near(actual, expected: float, label: str, tol: float = 4.0) -> None:
    if actual is None or abs(float(actual) - expected) > tol:
        raise AssertionError(f"{label}: expected ~{expected}, got {actual}")
    print(f"  ok: {label} (~{expected})")


def body() -> None:
    with RpcSubprocess("hello-raw-pointer") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None,
            source="paint",
            viewport=WIN,
            desc="pane surface resolved",
        )

        # --- boot: no lean, no position → no marker ---
        assert_eq(tf.query(f"{EXT}/tilt_x"), 0.0, "boot: zero tilt_x")
        assert_eq(tf.query(f"{EXT}/tilt_y"), 0.0, "boot: zero tilt_y")
        assert tip_center(snap) is None, "boot: no tip marker without a position"

        # --- position the pointer over the pane so a tilt change has a surface to
        #     reach (out-of-band tilt is delivered to the hover target, exactly
        #     like a pen resting on the tablet). The tip appears ON the cursor at
        #     zero tilt. ---
        tf.hover(at=CENTER)
        snap = wait_snap(
            tf,
            lambda s: tip_center(s) is not None,
            source="paint",
            viewport=WIN,
            desc="the tip marker appears on hover",
        )
        cx0, cy0 = tip_center(snap)
        assert_near(cx0, CENTER[0], "tip x on the cursor at zero tilt", tol=2.0)
        assert_near(cy0, CENTER[1], "tip y on the cursor at zero tilt", tol=2.0)

        # --- a driven rightward tilt is delivered at once (no move required) and
        #     leans the tip right; a pure tilt_x does not move it vertically. ---
        tf.pointer_tilt(60.0, 0.0)
        wait_query(tf, f"{EXT}/tilt_x", 60.0, desc="tilt_x 60 delivered")
        assert_eq(tf.query(f"{EXT}/tilt_y"), 0.0, "tilt_y stays zero")
        snap = wait_snap(
            tf,
            lambda s: (tip_center(s) or (0, 0))[0] > cx0 + 10,
            source="paint",
            viewport=WIN,
            desc="the tip leans right under tilt_x",
        )
        cx_r, cy_r = tip_center(snap)
        assert cx_r > cx0, f"tilt_x>0 leans the tip right: {cx_r} > {cx0}"
        # offset ≈ (60/90) * 40 = 26.7 px to the right of the cursor.
        assert_near(cx_r - cx0, (60.0 / 90.0) * TILT_SPAN, "rightward lean magnitude")
        assert_near(cy_r, cy0, "a pure tilt_x keeps the tip's y", tol=2.0)

        # --- returning to zero tilt puts the tip back on the cursor. ---
        tf.pointer_tilt(0.0, 0.0)
        wait_query(tf, f"{EXT}/tilt_x", 0.0, desc="tilt released to 0")
        snap = wait_snap(
            tf,
            lambda s: abs((tip_center(s) or (999, 0))[0] - cx0) < 2.0,
            source="paint",
            viewport=WIN,
            desc="the tip returns to the cursor at zero tilt",
        )
        assert_near(tip_center(snap)[0], cx0, "tip back on the cursor x", tol=2.0)

        # --- a downward tilt leans the tip down; a pure tilt_y keeps its x. ---
        tf.pointer_tilt(0.0, 60.0)
        wait_query(tf, f"{EXT}/tilt_y", 60.0, desc="tilt_y 60 delivered")
        snap = wait_snap(
            tf,
            lambda s: (tip_center(s) or (0, 0))[1] > cy0 + 10,
            source="paint",
            viewport=WIN,
            desc="the tip leans down under tilt_y",
        )
        cx_d, cy_d = tip_center(snap)
        assert cy_d > cy0, f"tilt_y>0 leans the tip down: {cy_d} > {cy0}"
        assert_near(cy_d - cy0, (60.0 / 90.0) * TILT_SPAN, "downward lean magnitude")
        assert_near(cx_d, cx0, "a pure tilt_y keeps the tip's x", tol=2.0)

        # --- a negative tilt leans the other way (left / up). ---
        tf.pointer_tilt(-60.0, 0.0)
        wait_query(tf, f"{EXT}/tilt_x", -60.0, desc="tilt_x -60 delivered")
        snap = wait_snap(
            tf,
            lambda s: (tip_center(s) or (999, 0))[0] < cx0 - 10,
            source="paint",
            viewport=WIN,
            desc="the tip leans left under negative tilt_x",
        )
        assert tip_center(snap)[0] < cx0, "tilt_x<0 leans the tip left"

        # --- out-of-range tilt clamps to the axis limit at the router (widest
        #     lean); 120° becomes 90°, a 40px offset. ---
        tf.pointer_tilt(120.0, 0.0)
        wait_query(tf, f"{EXT}/tilt_x", 90.0, desc="out-of-range tilt clamps to +90")
        snap = wait_snap(
            tf,
            lambda s: (tip_center(s) or (0, 0))[0] > cx0 + 30,
            source="paint",
            viewport=WIN,
            desc="the clamped lean is the widest",
        )
        assert_near(tip_center(snap)[0] - cx0, TILT_SPAN, "full lean magnitude")

        # --- tilt RIDES a move: set a lean, then move — it persists at the new
        #     position and the tip follows the cursor + its offset. ---
        tf.pointer_tilt(45.0, 0.0)
        wait_query(tf, f"{EXT}/tilt_x", 45.0, desc="tilt set to 45")
        tf.hover(at=RIGHT)
        snap = wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.8) < 0.03,
            source="paint",
            viewport=WIN,
            desc="the pointer moves right under held tilt",
        )
        assert_eq(tf.query(f"{EXT}/tilt_x"), 45.0, "tilt persists across the move")
        cx_move, _ = tip_center(snap)
        assert cx_move > CENTER[0], f"the tip followed the cursor right, x={cx_move}"

        # --- leaving the pane drops the position, so the tip cannot be placed. ---
        tf.hover(at=ABOVE)
        snap = wait_snap(
            tf,
            lambda s: tip_center(s) is None,
            source="paint",
            viewport=WIN,
            desc="the tip vanishes when the pointer leaves the pane",
        )
        assert tip_center(snap) is None, "no tip off the pane (no position to place it)"

        # --- the wire contract: scene/pointer_tilt rejects a missing / bad axis
        #     (both axes are required; a typo surfaces at the call). ---
        try:
            tf.request("scene/pointer_tilt", {})
            raise AssertionError("missing both axes must be rejected")
        except RpcError as exc:
            print(f"  ok: missing axes rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_tilt", {"tilt_x": 10.0})
            raise AssertionError("a missing tilt_y must be rejected")
        except RpcError as exc:
            print(f"  ok: missing tilt_y rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_tilt", {"tilt_x": "left", "tilt_y": 0.0})
            raise AssertionError("a non-numeric axis must be rejected")
        except RpcError as exc:
            print(f"  ok: non-numeric axis rejected ({exc.message!r})")

        # --- recovery: a valid tilt after the error probes still reads and paints. ---
        tf.hover(at=CENTER)
        tf.pointer_tilt(20.0, -10.0)
        wait_query(tf, f"{EXT}/tilt_x", 20.0, desc="a real tilt recovers after the errors")
        assert_eq(tf.query(f"{EXT}/tilt_y"), -10.0, "tilt_y recovered too")
        snap = wait_snap(
            tf,
            lambda s: tip_center(s) is not None,
            source="paint",
            viewport=WIN,
            desc="the tip is painted again after recovery",
        )
        cx_rec, cy_rec = tip_center(snap)
        assert cx_rec > CENTER[0], "the recovered rightward lean offsets the tip right"
        assert cy_rec < CENTER[1], "the recovered upward lean offsets the tip up"


if __name__ == "__main__":
    sys.exit(run_demo("r1429_tilt", body))
