#!/usr/bin/env python3
"""R1430 §5.35 §5.15 — the remaining Qt QTabletEvent scalar axes on the raw surface.

R1423 added pressure and R1429 tilt. R1430 completes the Qt `QTabletEvent` scalar
axis set: TWIST (barrel rotation, W3C `PointerEvent.twist` / Qt `rotation()`),
TANGENTIAL PRESSURE (airbrush finger wheel, W3C `tangentialPressure` / Qt
`tangentialPressure()`), and HEIGHT (hover distance above the tablet, Qt `z()`).
Each is a positionless out-of-band axis driven by its own `scene/pointer_*` RPC
(§2 #2) — winit exposes none of them, so the RPC is the sole driver and every axis
is exercisable headless with no tablet.

The runtime stores all five axes (pressure/tilt/twist/tangential/height) in ONE
bundle and forwards them WHOLE on every move, so adding an axis is a struct field,
not a fifth copy of the plumbing.

hello-raw-pointer paints each axis: the twist orbits an orientation dot around the
pen tip, the tangential fills a finger-wheel bar, and the height shrinks the pen-tip
marker. This demo drives all three over the wire and reads BOTH the introspect
fields AND the computed rects.

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1430_tablet_axes.py
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
ORBIT = "twist.orbit"
TANG = "tang.bar"
EXT = "/external"

RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104


def pane_at(fx: float, fy: float) -> tuple[float, float]:
    return (RECT_X + fx * RECT_W, RECT_Y + fy * RECT_H)


CENTER = pane_at(0.5, 0.5)


def rect_of(snap, tag):
    node = find_by_tag(snap, tag)
    return node.get("rect") if node else None


def center_of(snap, tag):
    r = rect_of(snap, tag)
    return None if r is None else (r["x"] + r["w"] / 2.0, r["y"] + r["h"] / 2.0)


def body() -> None:
    with RpcSubprocess("hello-raw-pointer") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None,
            source="paint",
            viewport=WIN,
            desc="pane surface resolved",
        )

        # --- boot: all three axes at rest ---
        assert_eq(tf.query(f"{EXT}/twist"), 0.0, "boot: zero twist")
        assert_eq(tf.query(f"{EXT}/tangential"), 0.0, "boot: neutral tangential")
        assert_eq(tf.query(f"{EXT}/height"), 0.0, "boot: zero height")

        # The finger-wheel bar is always shown; at rest (tangential 0) it is
        # half-full.
        bar_rest = rect_of(snap, TANG)
        assert bar_rest is not None, "the tangential bar is painted at boot"
        w_rest = bar_rest["w"]
        assert w_rest > 0, f"the bar has a positive rest width, got {w_rest}"

        # --- position the pointer over the pane so the tip / orbit have a place. ---
        tf.hover(at=CENTER)
        snap = wait_snap(
            tf,
            lambda s: center_of(s, TIP) is not None,
            source="paint",
            viewport=WIN,
            desc="the pen tip appears on hover",
        )
        tip_c = center_of(snap, TIP)
        assert tip_c is not None, "the tip is placed on hover"

        # === TWIST (barrel rotation) ===
        # twist 0 orbits the orientation dot ABOVE the tip.
        orbit0 = center_of(snap, ORBIT)
        assert orbit0 is not None, "the orbit dot is present on hover"
        assert orbit0[1] < tip_c[1], "twist 0 orbits above the tip"

        # twist 90 swings the orbit to the RIGHT of the tip (clockwise).
        tf.pointer_twist(90.0)
        wait_query(tf, f"{EXT}/twist", 90.0, desc="twist 90 delivered")
        snap = wait_snap(
            tf,
            lambda s: (center_of(s, ORBIT) or (0, 0))[0] > tip_c[0] + 8,
            source="paint",
            viewport=WIN,
            desc="twist 90 swings the orbit right",
        )
        orbit90 = center_of(snap, ORBIT)
        assert orbit90[0] > tip_c[0], "twist 90 orbits right of the tip"
        assert orbit90[1] > orbit0[1], "twist 90 sits lower than twist 0 (clockwise)"

        # twist wraps: 450 -> 90 (an angle folds, it does not clamp).
        tf.pointer_twist(450.0)
        wait_query(tf, f"{EXT}/twist", 90.0, desc="twist 450 wraps to 90")

        # twist 180 puts the orbit BELOW the tip.
        tf.pointer_twist(180.0)
        wait_query(tf, f"{EXT}/twist", 180.0, desc="twist 180 delivered")
        snap = wait_snap(
            tf,
            lambda s: (center_of(s, ORBIT) or (0, 0))[1] > tip_c[1] + 8,
            source="paint",
            viewport=WIN,
            desc="twist 180 swings the orbit below",
        )
        assert center_of(snap, ORBIT)[1] > tip_c[1], "twist 180 orbits below the tip"
        tf.pointer_twist(0.0)
        wait_query(tf, f"{EXT}/twist", 0.0, desc="twist released")

        # === TANGENTIAL PRESSURE (airbrush wheel) ===
        # +1 fills the bar past its rest, -1 empties it below rest.
        tf.pointer_tangential_pressure(1.0)
        wait_query(tf, f"{EXT}/tangential", 1.0, desc="tangential +1 delivered")
        snap = wait_snap(
            tf,
            lambda s: (rect_of(s, TANG) or {}).get("w", 0) > w_rest,
            source="paint",
            viewport=WIN,
            desc="tangential +1 fills the bar",
        )
        w_full = rect_of(snap, TANG)["w"]
        assert w_full > w_rest, f"+1 fills past rest: {w_full} > {w_rest}"

        tf.pointer_tangential_pressure(-1.0)
        wait_query(tf, f"{EXT}/tangential", -1.0, desc="tangential -1 delivered")
        snap = wait_snap(
            tf,
            lambda s: (rect_of(s, TANG) or {}).get("w", 999) < w_rest,
            source="paint",
            viewport=WIN,
            desc="tangential -1 empties the bar",
        )
        w_low = rect_of(snap, TANG)["w"]
        assert w_low < w_rest, f"-1 empties below rest: {w_low} < {w_rest}"

        # out of range clamps to +1 at the router (the fullest bar).
        tf.pointer_tangential_pressure(3.0)
        wait_query(tf, f"{EXT}/tangential", 1.0, desc="tangential 3 clamps to 1")
        tf.pointer_tangential_pressure(0.0)
        wait_query(tf, f"{EXT}/tangential", 0.0, desc="tangential released to rest")

        # === HEIGHT (Qt z() hover distance) ===
        # a lifted pen paints a SMALLER tip than one in contact.
        tip_contact = rect_of(snap, TIP)["w"]
        tf.pointer_height(30.0)
        wait_query(tf, f"{EXT}/height", 30.0, desc="height 30 delivered")
        snap = wait_snap(
            tf,
            lambda s: (rect_of(s, TIP) or {}).get("w", 999) < tip_contact,
            source="paint",
            viewport=WIN,
            desc="a lifted pen shrinks the tip",
        )
        tip_lifted = rect_of(snap, TIP)["w"]
        assert tip_lifted < tip_contact, f"lifted tip is smaller: {tip_lifted} < {tip_contact}"

        # height floors at 0 (a distance is non-negative).
        tf.pointer_height(-5.0)
        wait_query(tf, f"{EXT}/height", 0.0, desc="negative height floors at 0")

        # === axes are INDEPENDENT and RIDE a move together ===
        tf.pointer_twist(45.0)
        tf.pointer_tangential_pressure(0.5)
        tf.pointer_height(10.0)
        wait_query(tf, f"{EXT}/twist", 45.0, desc="twist set for the combined check")
        assert_eq(tf.query(f"{EXT}/tangential"), 0.5, "tangential held independently")
        assert_eq(tf.query(f"{EXT}/height"), 10.0, "height held independently")
        tf.hover(at=pane_at(0.7, 0.5))
        snap = wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.7) < 0.03,
            source="paint",
            viewport=WIN,
            desc="the pointer moves under held tablet axes",
        )
        assert_eq(tf.query(f"{EXT}/twist"), 45.0, "twist persisted across the move")
        assert_eq(tf.query(f"{EXT}/tangential"), 0.5, "tangential persisted")
        assert_eq(tf.query(f"{EXT}/height"), 10.0, "height persisted")

        # === wire contract: each rejects a missing / non-numeric value ===
        for method, key in [
            ("scene/pointer_twist", "twist"),
            ("scene/pointer_tangential_pressure", "tangential"),
            ("scene/pointer_height", "height"),
        ]:
            try:
                tf.request(method, {})
                raise AssertionError(f"{method}: missing value must be rejected")
            except RpcError as exc:
                print(f"  ok: {method} missing value rejected ({exc.message!r})")
            try:
                tf.request(method, {key: "x"})
                raise AssertionError(f"{method}: non-numeric must be rejected")
            except RpcError as exc:
                print(f"  ok: {method} non-numeric rejected ({exc.message!r})")

        # recovery: a valid drive after the error probes still reads.
        tf.pointer_twist(120.0)
        wait_query(tf, f"{EXT}/twist", 120.0, desc="a real twist recovers after errors")


if __name__ == "__main__":
    sys.exit(run_demo("r1430_tablet_axes", body))
