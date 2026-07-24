#!/usr/bin/env python3
"""R1433 §5.35 §5.15 — a native ROTATION gesture spins a gizmo.

`External::rotation_gesture` forwards the Qt `QNativeGestureEvent`
`RotateNativeGesture` / macOS `rotateWithEvent:` peer, the pinch sibling: the
INCREMENTAL rotation of a two-finger trackpad twist, in DEGREES (positive =
counter-clockwise, winit's convention), bracketed by a `GesturePhase` arc
(begin / update / end / cancel). The gizmo accumulates `angle += rotation`
across the arc, and — the whole point of the phase — DISCARDS the in-flight
rotation on `cancel`, reverting to the angle it held when the gesture began.

winit surfaces `WindowEvent::RotationGesture` only on macOS / iOS, so
`scene/rotation_gesture` is the sole driver headless (§2 #2): this demo rotates
the gizmo over the wire with no trackpad and reads BOTH the `angle_deg`
introspect field AND the needle's paint rect (§2 #7) — they move together, the
tip orbiting the hub.

Run from the workspace root:
    cargo build -p hello-rotate-gesture --release
    python3 tools/demos/r1433_rotation_gesture.py
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
PANE = "gizmo"
NEEDLE = "rotate.needle"
EXT = "/external"

# The pane + hub geometry mirrors the Rust constants: hub at the pane centre,
# the needle tip orbits it at NEEDLE_R.
RECT_X, RECT_Y, RECT_W, RECT_H = 20, 56, WIN[0] - 40, WIN[1] - 120
HUB = (RECT_X + 0.5 * RECT_W, RECT_Y + 0.5 * RECT_H)  # (320, 216)
NEEDLE_R = 100.0
CENTER = HUB
RIGHT = (RECT_X + 0.8 * RECT_W, RECT_Y + 0.5 * RECT_H)


def needle_center(snap):
    node = find_by_tag(snap, NEEDLE)
    if not node:
        return None
    r = node["rect"]
    return (r["x"] + r["w"] / 2.0, r["y"] + r["h"] / 2.0)


def angle_of(tf) -> float:
    return float(tf.query(f"{EXT}/angle_deg"))


def assert_close(got: float, want: float, label: str) -> None:
    assert abs(got - want) < 1e-6, f"{label}: expected {want}, got {got}"
    print(f"  ok: {label} ({got})")


def body() -> None:
    with RpcSubprocess("hello-rotate-gesture") as tf:
        base_snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None and needle_center(s) is not None,
            source="paint",
            viewport=WIN,
            desc="gizmo + needle resolved",
        )

        # --- boot: angle 0, no gesture yet; the tip sits to the RIGHT of the hub. ---
        assert_close(angle_of(tf), 0.0, "boot angle is 0.0")
        assert_eq(tf.query(f"{EXT}/phase"), None, "boot: no gesture phase yet")
        assert_eq(tf.query(f"{EXT}/events"), 0, "boot: zero rotation events")
        boot_tip = needle_center(base_snap)
        assert boot_tip[0] > HUB[0] + 50, f"boot tip is right of the hub: {boot_tip}"
        assert abs(boot_tip[1] - HUB[1]) <= 2, f"boot tip is level with the hub: {boot_tip}"
        print(f"  ok: boot needle tip right of hub {boot_tip}")

        # --- a Begin..Update arc rotates CCW; the phase brackets the arc. ---
        tf.rotation_gesture(0.0, "begin", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "begin", "phase is begin")
        assert_eq(tf.query(f"{EXT}/events"), 1, "one rotation event")
        assert_close(angle_of(tf), 0.0, "begin delta 0 leaves angle at 0.0")

        tf.rotation_gesture(45.0, "update", at=CENTER)
        assert_close(angle_of(tf), 45.0, "update +45 -> angle 45")
        assert_eq(tf.query(f"{EXT}/phase"), "update", "phase is update")
        assert_eq(tf.query(f"{EXT}/events"), 2, "two rotation events")
        turned = wait_snap(
            tf,
            lambda s: (nc := needle_center(s)) is not None
            and nc[1] < boot_tip[1] - 5
            and nc[0] < boot_tip[0] - 5,
            source="paint",
            viewport=WIN,
            desc="needle lifts up-and-left with the CCW turn (paint tracks the field)",
        )
        print(f"  ok: +45 needle tip {needle_center(turned)} lifted from {boot_tip}")

        # --- a second +45 Update ADDS (45 + 45 = 90); tip is above the hub. ---
        tf.rotation_gesture(45.0, "update", at=CENTER)
        assert_close(angle_of(tf), 90.0, "additive update -> angle 90")
        top = wait_snap(
            tf,
            lambda s: (nc := needle_center(s)) is not None
            and abs(nc[0] - HUB[0]) <= 2
            and nc[1] < HUB[1] - 50,
            source="paint",
            viewport=WIN,
            desc="at 90deg the tip is centred above the hub",
        )
        print(f"  ok: +90 needle tip above hub {needle_center(top)}")

        # --- End COMMITS: the accumulated angle stays. ---
        tf.rotation_gesture(0.0, "end", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "end", "phase is end")
        assert_close(angle_of(tf), 90.0, "end commits angle 90")

        # --- a second arc: rotate on to 180deg, then CANCEL back to 90. ---
        tf.rotation_gesture(0.0, "begin", at=CENTER)
        assert_close(
            float(tf.query(f"{EXT}/angle_at_begin")), 90.0, "begin snapshots 90 for cancel"
        )
        tf.rotation_gesture(90.0, "update", at=CENTER)
        assert_close(angle_of(tf), 180.0, "mid-arc angle 180")
        left = wait_snap(
            tf,
            lambda s: (nc := needle_center(s)) is not None and nc[0] < HUB[0] - 50,
            source="paint",
            viewport=WIN,
            desc="at 180deg the tip swings to the LEFT of the hub",
        )
        print(f"  ok: 180 needle tip left of hub {needle_center(left)}")
        tf.rotation_gesture(0.0, "cancel", at=CENTER)
        assert_eq(tf.query(f"{EXT}/phase"), "cancel", "phase is cancel")
        assert_close(angle_of(tf), 90.0, "cancel discards the in-flight rotation -> 90")
        reverted = wait_snap(
            tf,
            lambda s: (nc := needle_center(s)) is not None
            and abs(nc[0] - HUB[0]) <= 2
            and nc[1] < HUB[1] - 50,
            source="paint",
            viewport=WIN,
            desc="the needle snaps back above the hub (committed 90)",
        )
        print(f"  ok: cancel needle tip back above hub {needle_center(reverted)}")

        # --- a negative delta rotates CLOCKWISE (tip drops below the hub). ---
        tf.invoke(f"{EXT}/reset", None)
        assert_close(angle_of(tf), 0.0, "reset returns to base angle")
        tf.rotation_gesture(0.0, "begin", at=CENTER)
        tf.rotation_gesture(-90.0, "update", at=CENTER)
        assert_close(angle_of(tf), -90.0, "clockwise -90 -> angle -90")
        below = wait_snap(
            tf,
            lambda s: (nc := needle_center(s)) is not None and nc[1] > HUB[1] + 50,
            source="paint",
            viewport=WIN,
            desc="at -90deg the tip drops below the hub",
        )
        print(f"  ok: -90 needle tip below hub {needle_center(below)}")

        # --- anchor: a rotation off-centre records where it happened (x_rel). ---
        tf.invoke(f"{EXT}/reset", None)
        tf.rotation_gesture(5.0, "begin", at=RIGHT)
        assert_close(
            float(tf.query(f"{EXT}/anchor_x")), 0.8, "anchor x fraction is 0.8 (right of pane)"
        )
        assert_close(
            float(tf.query(f"{EXT}/anchor_y")), 0.5, "anchor y fraction is 0.5 (vertical centre)"
        )

        # --- modifiers ride the out-of-band cache (Qt Shift-snap parity). ---
        tf.modifiers(shift=True)
        tf.rotation_gesture(5.0, "update", at=CENTER)
        assert "s" in str(tf.query(f"{EXT}/last_mods")), "shift reached the rotation hook"
        tf.modifiers()  # release
        tf.rotation_gesture(5.0, "update", at=CENTER)
        assert_eq(tf.query(f"{EXT}/last_mods"), "", "modifiers cleared on release")

        # --- reset: back to the base, gesture history cleared. ---
        tf.invoke(f"{EXT}/reset", None)
        assert_close(angle_of(tf), 0.0, "reset angle 0.0")
        assert_eq(tf.query(f"{EXT}/events"), 0, "reset clears the event count")
        assert_eq(tf.query(f"{EXT}/phase"), None, "reset clears the phase")

        # --- wire contract: a bad rotation / phase rejects, enqueues nothing. ---
        for params, why in [
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "rotation": 10.0, "phase": "started"},
             "unknown phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "rotation": 10.0, "phase": 5},
             "non-string phase"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "phase": "begin"},
             "missing rotation"),
            ({"at": {"x": CENTER[0], "y": CENTER[1]}, "rotation": 10.0},
             "missing phase"),
        ]:
            try:
                tf.request("scene/rotation_gesture", params)
                raise AssertionError(f"{why} must be rejected")
            except RpcError as exc:
                print(f"  ok: {why} rejected ({exc.message!r})")

        # --- recovery: a valid rotation after the errors still spins the gizmo. ---
        tf.rotation_gesture(0.0, "begin", at=CENTER)
        tf.rotation_gesture(30.0, "update", at=CENTER)
        assert_close(angle_of(tf), 30.0, "a real rotation recovers after the rejects")


if __name__ == "__main__":
    sys.exit(run_demo("r1433_rotation_gesture", body))
