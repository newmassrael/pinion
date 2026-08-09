#!/usr/bin/env python3
"""R1432 §5.35 §5.15 — a native PINCH (magnify) gesture zooms a viewport.

`External::pinch_gesture` forwards the toolkit native gesture event `ZoomNativeGesture`
/ macOS `magnify:` peer: the INCREMENTAL magnification of a two-finger trackpad
pinch, bracketed by a `GesturePhase` arc (begin / update / end / cancel). The
viewport accumulates `scale *= 1.0 + magnification` across the arc, and — the
whole point of the phase — DISCARDS the in-flight zoom on `cancel`, reverting to
the scale it held when the gesture began.

winit surfaces `WindowEvent::PinchGesture` only on macOS / iOS, so
`scene/pinch_gesture` is the sole driver headless (§2 #2): this demo zooms the
viewport over the wire with no trackpad and reads BOTH the `scale` introspect
field AND the square's paint rect (§2 #7) — they move together.

Run from the workspace root:
    cargo build -p hello-pinch-zoom --release
    python3 tools/demos/r1432_pinch_gesture.py
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
    wait_snap,
)

WIN = (640, 440)
PANE = "viewport"
SQUARE = "zoom.square"
EXT = "/external"

RECT_X, RECT_Y, RECT_W, RECT_H = 20, 56, WIN[0] - 40, WIN[1] - 120
CENTER = (RECT_X + 0.5 * RECT_W, RECT_Y + 0.5 * RECT_H)
RIGHT = (RECT_X + 0.8 * RECT_W, RECT_Y + 0.5 * RECT_H)

BASE_SIDE = 48.0
MIN_SCALE, MAX_SCALE = 0.25, 6.0


def square_w(snap) -> int:
    node = find_by_tag(snap, SQUARE)
    return int(node["rect"]["w"]) if node else -1


def expected_side(scale: float) -> int:
    side = round(BASE_SIDE * scale)
    return max(1, min(side, min(RECT_W, RECT_H)))


def scale_of(tf) -> float:
    return float(tf.query(f"{EXT}/scale"))


def assert_close(got: float, want: float, label: str) -> None:
    assert abs(got - want) < 1e-6, f"{label}: expected {want}, got {got}"
    print(f"  ok: {label} ({got})")


def body() -> None:
    with RpcSubprocess("hello-pinch-zoom") as tf:
        base_snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None and square_w(s) == expected_side(1.0),
            source="paint",
            viewport=WIN,
            desc="viewport + base square resolved",
        )

        # --- boot: scale 1.0, no gesture yet, base square. ---
        assert_close(scale_of(tf), 1.0, "boot scale is 1.0")
        assert_eq(tf.query(f"{EXT}/phase"), None, "boot: no gesture phase yet")
        assert_eq(tf.query(f"{EXT}/events"), 0, "boot: zero pinch events")
        assert_eq(square_w(base_snap), expected_side(1.0), "boot square is base side")

        # --- a Begin..Update arc zooms IN; the phase brackets the arc. ---
        tf.pinch_gesture(0.0, "begin", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "begin", "phase is begin")
        assert_eq(tf.query(f"{EXT}/events"), 1, "one pinch event")
        assert_close(scale_of(tf), 1.0, "begin delta 0 leaves scale at 1.0")

        tf.pinch_gesture(0.5, "update", at=CENTER)
        assert_close(scale_of(tf), 1.5, "update +50% -> scale 1.5")
        assert_eq(tf.query(f"{EXT}/phase"), "update", "phase is update")
        assert_eq(tf.query(f"{EXT}/events"), 2, "two pinch events")
        zoomed = wait_snap(
            tf,
            lambda s: square_w(s) == expected_side(1.5),
            source="paint",
            viewport=WIN,
            desc="square grows with the zoom (paint tracks the field)",
        )
        assert square_w(zoomed) > square_w(base_snap), "the zoomed square is bigger"

        # --- a second Update compounds multiplicatively (1.5 * 2.0 = 3.0). ---
        tf.pinch_gesture(1.0, "update", at=CENTER)
        assert_close(scale_of(tf), 3.0, "compound update -> scale 3.0")
        wait_snap(
            tf,
            lambda s: square_w(s) == expected_side(3.0),
            source="paint",
            viewport=WIN,
            desc="square at 3x",
        )

        # --- End COMMITS: the accumulated scale stays. ---
        tf.pinch_gesture(0.0, "end", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "end", "phase is end")
        assert_close(scale_of(tf), 3.0, "end commits scale 3.0")

        # --- a second arc that CANCELS reverts to the committed 3.0. ---
        tf.pinch_gesture(0.0, "begin", at=CENTER)
        assert_close(
            float(tf.query(f"{EXT}/scale_at_begin")), 3.0, "begin snapshots 3.0 for cancel"
        )
        tf.pinch_gesture(0.5, "update", at=CENTER)
        assert_close(scale_of(tf), 4.5, "mid-arc zoom to 4.5")
        wait_snap(
            tf,
            lambda s: square_w(s) == expected_side(4.5),
            source="paint",
            viewport=WIN,
            desc="square grows mid-arc",
        )
        tf.pinch_gesture(0.0, "cancel", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "cancel", "phase is cancel")
        assert_close(scale_of(tf), 3.0, "cancel discards the in-flight zoom -> 3.0")
        reverted = wait_snap(
            tf,
            lambda s: square_w(s) == expected_side(3.0),
            source="paint",
            viewport=WIN,
            desc="the square snaps back to the committed size",
        )
        assert_eq(square_w(reverted), expected_side(3.0), "square reverted to 3x")

        # --- clamp: a huge zoom-in stops at MAX_SCALE, never runs away. ---
        tf.pinch_gesture(0.0, "begin", at=CENTER)
        tf.pinch_gesture(10.0, "update", at=CENTER)
        tf.pinch_gesture(0.0, "end", at=CENTER)
        assert_close(scale_of(tf), MAX_SCALE, "zoom-in clamps at MAX_SCALE")
        wait_snap(
            tf,
            lambda s: square_w(s) == expected_side(MAX_SCALE),
            source="paint",
            viewport=WIN,
            desc="square at max zoom",
        )

        # --- clamp: a huge zoom-out stops at MIN_SCALE. ---
        tf.pinch_gesture(0.0, "begin", at=CENTER)
        tf.pinch_gesture(-0.99, "update", at=CENTER)
        tf.pinch_gesture(-0.99, "update", at=CENTER)
        tf.pinch_gesture(0.0, "end", at=CENTER)
        assert_close(scale_of(tf), MIN_SCALE, "zoom-out clamps at MIN_SCALE")

        # --- anchor: a pinch off-centre records where it happened (x_rel). ---
        tf.invoke(f"{EXT}/reset", None)
        assert_close(scale_of(tf), 1.0, "reset returns to base scale")
        tf.pinch_gesture(0.25, "begin", at=RIGHT)
        assert_close(
            float(tf.query(f"{EXT}/anchor_x")), 0.8, "anchor x fraction is 0.8 (right of pane)"
        )
        assert_close(
            float(tf.query(f"{EXT}/anchor_y")), 0.5, "anchor y fraction is 0.5 (vertical centre)"
        )

        # --- modifiers ride the out-of-band cache (the toolkit Ctrl-fine-zoom
        # parity). ---
        tf.modifiers(ctrl=True)
        tf.pinch_gesture(0.1, "update", at=CENTER)
        assert "c" in str(tf.query(f"{EXT}/last_mods")), "ctrl reached the pinch hook"
        tf.modifiers()  # release
        tf.pinch_gesture(0.1, "update", at=CENTER)
        assert_eq(tf.query(f"{EXT}/last_mods"), "", "modifiers cleared on release")

        # --- reset: back to the base, gesture history cleared. ---
        tf.invoke(f"{EXT}/reset", None)
        assert_close(scale_of(tf), 1.0, "reset scale 1.0")
        assert_eq(tf.query(f"{EXT}/events"), 0, "reset clears the event count")
        assert_eq(tf.query(f"{EXT}/phase"), None, "reset clears the phase")

        # --- wire contract: a bad magnification / phase rejects, enqueues nothing.
        for params, why in [
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "magnification": 0.1, "phase": "started"},
             "unknown phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "magnification": 0.1, "phase": 5},
             "non-string phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "phase": "begin"},
             "missing magnification"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "magnification": 0.1},
             "missing phase"),
        ]:
            try:
                tf.request("scene/pinch_gesture", params)
                raise AssertionError(f"{why} must be rejected")
            except RpcError as exc:
                print(f"  ok: {why} rejected ({exc.message!r})")

        # --- recovery: a valid pinch after the errors still drives the zoom. ---
        tf.pinch_gesture(0.0, "begin", at=CENTER)
        tf.pinch_gesture(1.0, "update", at=CENTER)
        assert_close(scale_of(tf), 2.0, "a real pinch recovers after the rejects")


if __name__ == "__main__":
    sys.exit(run_demo("r1432_pinch_gesture", body))
