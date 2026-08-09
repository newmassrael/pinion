#!/usr/bin/env python3
"""R1434 §5.35 §5.15 — a native PAN gesture slides a map.

`External::pan_gesture` forwards the toolkit native gesture event
`PanNativeGesture` / winit `WindowEvent::PanGesture` peer, the pinch sibling
with a TWO-dimensional payload: the INCREMENTAL pan of an N-finger trackpad
gesture, in LOGICAL PIXELS on each axis, bracketed by a `GesturePhase` arc
(begin / update / end / cancel). The viewport accumulates `offset += delta`
across the arc, saturates at the content bound, and — the whole point of the
phase — DISCARDS the in-flight pan on `cancel`, snapping back to the offset it
held when the gesture began.

The delta keeps the platform's own sign: a native pan is direct manipulation
(the content follows the fingers), NOT the sign-flipped scroll command a wheel
carries.

winit surfaces `WindowEvent::PanGesture` only on iOS, so `scene/pan_gesture` is
the sole driver headless (§2 #2): this demo slides the map over the wire with no
trackpad and reads BOTH the `offset_x` / `offset_y` introspect fields AND the
marker's paint rect (§2 #7) — they move together, the content translating
rigidly.

Run from the workspace root:
    cargo build -p hello-pan-gesture --release
    python3 tools/demos/r1434_pan_gesture.py
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
PANE = "map"
MARKER = "pan.marker"
EXT = "/external"

# The pane geometry mirrors the Rust constants: the marker rests at the pane
# centre and the offset saturates at ±MAX_PAN.
RECT_X, RECT_Y, RECT_W, RECT_H = 20, 56, WIN[0] - 40, WIN[1] - 120
HOME = (RECT_X + 0.5 * RECT_W, RECT_Y + 0.5 * RECT_H)  # (320, 216)
MAX_PAN = 120.0
CENTER = HOME
RIGHT = (RECT_X + 0.8 * RECT_W, RECT_Y + 0.5 * RECT_H)


def marker_center(snap):
    node = find_by_tag(snap, MARKER)
    if not node:
        return None
    r = node["rect"]
    return (r["x"] + r["w"] / 2.0, r["y"] + r["h"] / 2.0)


def offset_of(tf) -> tuple[float, float]:
    return (float(tf.query(f"{EXT}/offset_x")), float(tf.query(f"{EXT}/offset_y")))


def assert_close(got: float, want: float, label: str) -> None:
    assert abs(got - want) < 1e-6, f"{label}: expected {want}, got {got}"
    print(f"  ok: {label} ({got})")


def assert_offset(tf, want: tuple[float, float], label: str) -> None:
    got = offset_of(tf)
    assert abs(got[0] - want[0]) < 1e-6 and abs(got[1] - want[1]) < 1e-6, (
        f"{label}: expected {want}, got {got}"
    )
    print(f"  ok: {label} {got}")


def body() -> None:
    with RpcSubprocess("hello-pan-gesture") as tf:
        base_snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None and marker_center(s) is not None,
            source="paint",
            viewport=WIN,
            desc="map + marker resolved",
        )

        # --- boot: offset (0, 0), no gesture yet; the marker rests at home. ---
        assert_offset(tf, (0.0, 0.0), "boot offset is the origin")
        assert_eq(tf.query(f"{EXT}/phase"), None, "boot: no gesture phase yet")
        assert_eq(tf.query(f"{EXT}/events"), 0, "boot: zero pan events")
        home_pin = marker_center(base_snap)
        assert abs(home_pin[0] - HOME[0]) <= 2, f"boot marker centred in x: {home_pin}"
        assert abs(home_pin[1] - HOME[1]) <= 2, f"boot marker centred in y: {home_pin}"
        print(f"  ok: boot marker at home {home_pin}")

        # --- a Begin..Update arc slides the content; the phase brackets it. ---
        tf.pan_gesture(0.0, 0.0, "begin", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "begin", "phase is begin")
        assert_eq(tf.query(f"{EXT}/events"), 1, "one pan event")
        assert_offset(tf, (0.0, 0.0), "begin delta 0 leaves the map at home")

        # X ONLY: the axes are independent — a horizontal pan must not move y.
        tf.pan_gesture(60.0, 0.0, "update", at=CENTER)
        assert_offset(tf, (60.0, 0.0), "update +60x -> x moves, y untouched")
        assert_eq(tf.query(f"{EXT}/phase"), "update", "phase is update")
        assert_eq(tf.query(f"{EXT}/events"), 2, "two pan events")
        slid_x = wait_snap(
            tf,
            lambda s: (mc := marker_center(s)) is not None
            and mc[0] > home_pin[0] + 50
            and abs(mc[1] - home_pin[1]) <= 2,
            source="paint",
            viewport=WIN,
            desc="marker slides right only (paint tracks the field)",
        )
        print(f"  ok: +60x marker {marker_center(slid_x)} from {home_pin}")

        # Y ONLY, negative: up the screen. Both axes accumulate independently.
        tf.pan_gesture(0.0, -40.0, "update", at=CENTER)
        assert_offset(tf, (60.0, -40.0), "update -40y -> y moves, x holds")
        slid_y = wait_snap(
            tf,
            lambda s: (mc := marker_center(s)) is not None and mc[1] < home_pin[1] - 30,
            source="paint",
            viewport=WIN,
            desc="marker lifts up with the negative y delta",
        )
        print(f"  ok: -40y marker {marker_center(slid_y)}")

        # A second x delta ADDS (60 + 30 = 90), it does not replace.
        tf.pan_gesture(30.0, 0.0, "update", at=CENTER)
        assert_offset(tf, (90.0, -40.0), "additive x update -> 90")

        # --- End COMMITS: the accumulated offset stays. ---
        tf.pan_gesture(0.0, 0.0, "end", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "end", "phase is end")
        assert_offset(tf, (90.0, -40.0), "end commits the offset")

        # --- a second arc: slide away, then CANCEL back to the committed spot. ---
        tf.pan_gesture(0.0, 0.0, "begin", at=CENTER)
        assert_close(
            float(tf.query(f"{EXT}/offset_at_begin_x")), 90.0, "begin snapshots x for cancel"
        )
        assert_close(
            float(tf.query(f"{EXT}/offset_at_begin_y")), -40.0, "begin snapshots y for cancel"
        )
        tf.pan_gesture(-150.0, 100.0, "update", at=CENTER)
        assert_offset(tf, (-60.0, 60.0), "mid-arc the map is somewhere else")
        moved = wait_snap(
            tf,
            lambda s: (mc := marker_center(s)) is not None
            and mc[0] < HOME[0] - 30
            and mc[1] > HOME[1] + 30,
            source="paint",
            viewport=WIN,
            desc="mid-arc the marker sits down-and-left of home",
        )
        print(f"  ok: mid-arc marker {marker_center(moved)}")
        tf.pan_gesture(0.0, 0.0, "cancel", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "cancel", "phase is cancel")
        assert_offset(tf, (90.0, -40.0), "cancel discards the in-flight pan")
        reverted = wait_snap(
            tf,
            lambda s: (mc := marker_center(s)) is not None
            and mc[0] > home_pin[0] + 50
            and mc[1] < home_pin[1] - 30,
            source="paint",
            viewport=WIN,
            desc="the marker snaps back to the committed offset",
        )
        print(f"  ok: cancel marker back at {marker_center(reverted)}")

        # --- the content bound: a huge pan saturates, it does not run away. ---
        tf.invoke(f"{EXT}/reset", None)
        assert_offset(tf, (0.0, 0.0), "reset recentres the map")
        tf.pan_gesture(0.0, 0.0, "begin", at=CENTER)
        tf.pan_gesture(900.0, 900.0, "update", at=CENTER)
        assert_offset(tf, (MAX_PAN, MAX_PAN), "offset saturates at the content bound")
        edge = wait_snap(
            tf,
            lambda s: (mc := marker_center(s)) is not None
            and abs(mc[0] - (HOME[0] + MAX_PAN)) <= 2
            and abs(mc[1] - (HOME[1] + MAX_PAN)) <= 2,
            source="paint",
            viewport=WIN,
            desc="the marker stops at the clamped edge, still inside the pane",
        )
        print(f"  ok: clamped marker {marker_center(edge)}")
        # The clamp is symmetric, and a saturated axis reverses immediately.
        tf.pan_gesture(-1000.0, -1000.0, "update", at=CENTER)
        assert_offset(tf, (-MAX_PAN, -MAX_PAN), "the clamp is symmetric")
        tf.pan_gesture(40.0, 0.0, "update", at=CENTER)
        assert_offset(tf, (-MAX_PAN + 40.0, -MAX_PAN), "a saturated axis reverses at once")

        # --- the last raw delta is readable (both axes, unclamped input). ---
        assert_close(float(tf.query(f"{EXT}/delta_x")), 40.0, "last delta_x is the raw value")
        assert_close(float(tf.query(f"{EXT}/delta_y")), 0.0, "last delta_y is the raw value")

        # --- anchor: a pan off-centre records where it happened (x_rel). ---
        tf.invoke(f"{EXT}/reset", None)
        tf.pan_gesture(5.0, 5.0, "begin", at=RIGHT)
        assert_close(
            float(tf.query(f"{EXT}/anchor_x")), 0.8, "anchor x fraction is 0.8 (right of pane)"
        )
        assert_close(
            float(tf.query(f"{EXT}/anchor_y")), 0.5, "anchor y fraction is 0.5 (vertical centre)"
        )

        # --- modifiers ride the out-of-band cache (the toolkit axis-lock
        # parity). ---
        tf.modifiers(shift=True)
        tf.pan_gesture(5.0, 0.0, "update", at=CENTER)
        assert "s" in str(tf.query(f"{EXT}/last_mods")), "shift reached the pan hook"
        tf.modifiers()  # release
        tf.pan_gesture(5.0, 0.0, "update", at=CENTER)
        assert_eq(tf.query(f"{EXT}/last_mods"), "", "modifiers cleared on release")

        # --- reset: back to home, gesture history cleared. ---
        tf.invoke(f"{EXT}/reset", None)
        assert_offset(tf, (0.0, 0.0), "reset offset is the origin")
        assert_eq(tf.query(f"{EXT}/events"), 0, "reset clears the event count")
        assert_eq(tf.query(f"{EXT}/phase"), None, "reset clears the phase")

        # --- wire contract: a bad axis / phase rejects, enqueues nothing. ---
        for params, why in [
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_x": 1.0, "delta_y": 1.0,
              "phase": "started"}, "unknown phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_x": 1.0, "delta_y": 1.0,
              "phase": 5}, "non-string phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_y": 1.0, "phase": "begin"},
             "missing delta_x"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_x": 1.0, "phase": "begin"},
             "missing delta_y"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_x": "1.0", "delta_y": 1.0,
              "phase": "begin"}, "non-numeric delta_x"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "delta_x": 1.0, "delta_y": 1.0},
             "missing phase"),
        ]:
            try:
                tf.request("scene/pan_gesture", params)
                raise AssertionError(f"{why} must be rejected")
            except RpcError as exc:
                print(f"  ok: {why} rejected ({exc.message!r})")
        assert_offset(tf, (0.0, 0.0), "no rejected call moved the map")

        # --- recovery: a valid pan after the errors still slides the map. ---
        tf.pan_gesture(0.0, 0.0, "begin", at=CENTER)
        tf.pan_gesture(25.0, -15.0, "update", at=CENTER)
        assert_offset(tf, (25.0, -15.0), "a real pan recovers after the rejects")


if __name__ == "__main__":
    sys.exit(run_demo("r1434_pan_gesture", body))
