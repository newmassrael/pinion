#!/usr/bin/env python3
"""R1423 §5.35 §5.15 — a raw pointer surface reacts to PRESSURE.

The R1416/R1418/R1422 raw sink carried the button, both edges, modifiers, the
held set, and the double-click — the full QMouseEvent. R1423 adds the ONE axis a
mouse does not have but a pen / touch does: FORCE. `External::pointer_pressure`
forwards the W3C `PointerEvent.pressure` / Qt `QTabletEvent::pressure()`
(normalised 0.0..=1.0) alongside each `pointer_move` and on a standalone change,
so a pressure-aware surface reads the live force.

The native source is the platform pen / touch force (winit `Touch::force`); the
AI-first source is `scene/pointer_pressure` (§2 #2) — positionless, out-of-band
like `scene/modifiers`, delivered to the surface under the pointer at once. So a
pressure-reactive surface is fully exercisable headless, no tablet required.

hello-raw-pointer paints a pressure-reactive INK DOT: a filled mark at the live
cursor whose diameter is `6 + pressure * 44` px — the DCC ink-brush vignette.
This demo drives the force over the wire and reads BOTH the `pressure` introspect
field AND the ink dot's computed rect (the mark grows with force, vanishes at
zero, follows the cursor).

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1423_pressure.py
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
INK = "ink.dot"
EXT = "/external"

# Mirror of the binding's PANE_RECT = Rect::new(16, 48, WIN_W - 32, WIN_H - 104).
RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104


def pane_at(fx: float, fy: float) -> tuple[float, float]:
    """The window pixel at fraction `(fx, fy)` across the pane rect."""
    return (RECT_X + fx * RECT_W, RECT_Y + fy * RECT_H)


CENTER = pane_at(0.5, 0.5)
RIGHT = pane_at(0.8, 0.5)
ABOVE = (360.0, 12.0)  # off the pane (above it)


def dot_rect(snap):
    """The ink mark's computed rect `{x, y, w, h}`, or `None` when absent."""
    node = find_by_tag(snap, INK)
    return node.get("rect") if node else None


def assert_near(actual, expected: float, label: str, tol: float = 0.03) -> None:
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

        # --- boot: no force, no ink mark ---
        assert_eq(tf.query(f"{EXT}/pressure"), 0.0, "boot: zero pressure")
        assert dot_rect(snap) is None, "boot: no ink mark at rest"

        # --- position the pointer over the pane so a pressure change has a
        #     surface to reach (out-of-band pressure is delivered to the hover
        #     target, exactly like a pen resting on the tablet). ---
        tf.hover(at=CENTER)
        wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.5) < 0.03,
            source="paint",
            viewport=WIN,
            desc="the pointer hovers the pane centre",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.5, "hover x")

        # --- a driven force is delivered at once (no move required) and paints
        #     the ink mark. ---
        tf.pointer_pressure(0.5)
        wait_query(tf, f"{EXT}/pressure", 0.5, desc="pressure 0.5 delivered")
        snap = wait_snap(
            tf,
            lambda s: dot_rect(s) is not None,
            source="paint",
            viewport=WIN,
            desc="the ink mark appears under force",
        )
        rect_half = dot_rect(snap)
        assert rect_half is not None, "the ink mark is painted under pressure"
        w_half = rect_half["w"]
        assert w_half > 0, f"the ink mark has a positive width, got {w_half}"
        # Centred on the cursor (CENTER ≈ (360, 216)); the mark straddles it.
        assert (
            rect_half["x"] < CENTER[0] < rect_half["x"] + rect_half["w"]
        ), "the mark is centred on the cursor x"

        # --- pressure SCALES the mark: heavier force → a wider mark. ---
        tf.pointer_pressure(0.75)
        wait_query(tf, f"{EXT}/pressure", 0.75, desc="pressure raised to 0.75")
        snap = wait_snap(
            tf,
            lambda s: (dot_rect(s) or {}).get("w", 0) > w_half,
            source="paint",
            viewport=WIN,
            desc="the ink mark grows with force",
        )
        w_heavy = dot_rect(snap)["w"]
        assert w_heavy > w_half, f"heavier force widens the mark: {w_heavy} > {w_half}"

        # --- zero force removes the mark (a lifted pen leaves no ink). ---
        tf.pointer_pressure(0.0)
        wait_query(tf, f"{EXT}/pressure", 0.0, desc="force released to 0")
        snap = wait_snap(
            tf,
            lambda s: dot_rect(s) is None,
            source="paint",
            viewport=WIN,
            desc="the ink mark vanishes at zero force",
        )
        assert dot_rect(snap) is None, "no mark at zero force"

        # --- out-of-range force clamps to 1.0 at the router (the mark is largest). ---
        tf.pointer_pressure(5.0)
        wait_query(tf, f"{EXT}/pressure", 1.0, desc="out-of-range force clamps to 1.0")
        snap = wait_snap(
            tf,
            lambda s: dot_rect(s) is not None,
            source="paint",
            viewport=WIN,
            desc="the clamped mark is painted",
        )
        w_max = dot_rect(snap)["w"]
        assert w_max >= w_heavy, f"full force is the widest mark: {w_max} >= {w_heavy}"

        # --- pressure RIDES a move: set a force, then move — the force persists
        #     at the new position and the mark follows the cursor. ---
        tf.pointer_pressure(0.25)
        wait_query(tf, f"{EXT}/pressure", 0.25, desc="force set to 0.25")
        tf.hover(at=RIGHT)
        snap = wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.8) < 0.03,
            source="paint",
            viewport=WIN,
            desc="the pointer moves right under held force",
        )
        assert_eq(tf.query(f"{EXT}/pressure"), 0.25, "force persists across the move")
        rect_right = dot_rect(snap)
        assert rect_right is not None, "the mark follows the cursor under held force"
        assert (
            rect_right["x"] > CENTER[0]
        ), f"the mark moved right with the cursor, x={rect_right['x']}"

        # --- leaving the pane drops the position, so the mark cannot be placed. ---
        tf.hover(at=ABOVE)
        snap = wait_snap(
            tf,
            lambda s: dot_rect(s) is None,
            source="paint",
            viewport=WIN,
            desc="the mark vanishes when the pointer leaves the pane",
        )
        assert dot_rect(snap) is None, "no mark off the pane (no position to place it)"

        # --- the wire contract: scene/pointer_pressure rejects a missing / bad
        #     value (a typo surfaces at the call). ---
        try:
            tf.request("scene/pointer_pressure", {})
            raise AssertionError("a missing value must be rejected")
        except RpcError as exc:
            print(f"  ok: missing value rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_pressure", {"value": "hard"})
            raise AssertionError("a non-numeric value must be rejected")
        except RpcError as exc:
            print(f"  ok: non-numeric value rejected ({exc.message!r})")

        # --- recovery: a valid force after the error probes still reads and paints. ---
        tf.hover(at=CENTER)
        tf.pointer_pressure(0.5)
        wait_query(tf, f"{EXT}/pressure", 0.5, desc="a real force recovers after the errors")
        snap = wait_snap(
            tf,
            lambda s: dot_rect(s) is not None,
            source="paint",
            viewport=WIN,
            desc="the mark is painted again after recovery",
        )
        assert dot_rect(snap) is not None, "the mark recovers"
        # The recovered mark (0.35) is between the released zero and full force.
        w_recover = dot_rect(snap)["w"]
        assert 0 < w_recover < w_max, f"the recovered mark is mid-size, got {w_recover}"


if __name__ == "__main__":
    sys.exit(run_demo("r1423_pressure", body))
