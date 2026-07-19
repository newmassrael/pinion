#!/usr/bin/env python3
"""R1373 §5.38 §5.40 §5.27 — inspector Details numeric drag-scrub (multi-object).

Drives `hello-inspector` via JSON-RPC. The multi-object Details panel gains the
iconic DCC interaction the property grid / data grid already have: press-and-drag
a common `Int` / `Float` row to scrub its value (the Blender / Unreal "drag the
number" gesture). The inspector's twist — its distinguishing dimension — is that
the scrub writes across the WHOLE selection RELATIVE to each object's own value,
so a MIXED numeric stays mixed but shifts together (the continuous peer of the
R1221 steppers). It rides the already-lifted `DragCalibration` substrate (R935
ruled the per-widget scrub glue divergent, so this is the 3rd consumer of the
substrate, not a lift), reusing the R51.34 pointer-capture lock + the stable-
pixel-basis idiom: `capture_normalize` names the `detail_panel` container (a
fixed-width rect), so the cursor-fraction delta recovers true pixel travel.

Witness (§2 #7 scene-as-data): the object model the view reads updates as the
cursor drags — observable through `/external/value.<i>` + `/external/mixed.<i>`
+ `/external/scrubbing`. The deterministic `scene/drag` RPC presses on the row,
marches the cursor through N interpolated frames under the capture lock (firing
the same `pointer_move` arc a real mouse does), and releases. The capture
substrate's native winit path is already proven end-to-end with live pixels by
r786 / r794; this round reuses it, so the deterministic phase is the gate.

  (A) boot — numeric kinds + seeded values; not scrubbing.
  (B) float scrub right — Speed (6.5) drags up by travel_px · 0.01.
  (C) float scrub left — drags back down; direction is signed.
  (D) int scrub — Health steps in whole units (8px / step).
  (E) int scrub left — steps back down.
  (F) MULTI-object scrub — Layer (Player 1 / Camera 1 / Light 2, mixed) shifts
      EACH by the same step; the mix is preserved (verified per object).
  (G) isolation — a scrub touches only its own row.
  (H) click (no move) — does not scrub and does not edit.
  (I) non-numeric drag — never scrubs (the bool still toggles on release).
  (J) double-click — still opens the inline type-in editor (the edit path is intact).

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-inspector"
INSPECTOR = "inspector"
VIEWPORT = (460, 420)

# The scrub sensitivities mirror the binding consts (DETAIL_PANEL_W = 270,
# SCRUB_FLOAT_PER_PX = 0.01, SCRUB_INT_PX_PER_STEP = 8).
REF_W = 270.0
FLOAT_PER_PX = 0.01
INT_PX_PER_STEP = 8.0

# Single-select Player common rows: Visible(0) Layer(1) Locked(2) Health(3)
# Speed(4) Team(5) Tint(6). Multi-select (all) common rows: Visible(0) Layer(1)
# Locked(2).
LAYER = 1
HEALTH = 3
SPEED = 4


def q(tf, key):
    return tf.query(f"/external/{key}")


def snap_rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def panel_width(tf) -> int:
    rects = snap_rects(tf)
    assert "detail_panel" in rects, "the Details panel container is present"
    return rects["detail_panel"][2]


def cell_rect(tf, tag: str):
    rects = snap_rects(tf)
    assert tag in rects, f"cell {tag} present in the paint scene"
    return rects[tag]


def scrub(tf, tag: str, dx: int) -> None:
    """Press the value cell `tag` at its centre (the label, between the steppers)
    and drag the captured cursor `dx` logical px horizontally (positive = right =
    increase)."""
    x, y, w, h = cell_rect(tf, tag)
    cx = x + w // 2
    cy = y + h // 2
    tf.drag(from_at=(float(cx), float(cy)), to_at=(float(cx + dx), float(cy)), steps=12)


def numeric_cell(row: int) -> str:
    return f"{INSPECTOR}#typein{row}"


def expected_float(base: float, pw: int, dx: int) -> float:
    """A dx-px cursor drag over a pw-wide panel moves x_rel by dx/pw, so
    travel_px = (dx/pw)·REF_W and the float moves travel_px·FLOAT_PER_PX."""
    return base + (dx / pw) * REF_W * FLOAT_PER_PX


def expected_int_steps(pw: int, dx: int) -> int:
    return round((dx / pw) * REF_W / INT_PX_PER_STEP)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")

        # ── (A) boot: select Player; numeric kinds + seeded values ──
        tf.invoke("/external/select", 0)
        wait_until(lambda: True if q(tf, "selection_count") == 1 else None,
                   desc="Player selected")
        pw = panel_width(tf)
        assert abs(pw - REF_W) <= 4, f"the Details panel paints at ~{REF_W}px (the scrub basis), got {pw}"
        assert_eq(q(tf, f"kind.{LAYER}"), "int", "Layer (row 1) is an int")
        assert_eq(q(tf, f"kind.{HEALTH}"), "int", "Health (row 3) is an int")
        assert_eq(q(tf, f"kind.{SPEED}"), "float", "Speed (row 4) is a float")
        assert_eq(q(tf, "kind.0"), "bool", "Visible (row 0) is a bool")
        assert_eq(q(tf, f"value.{LAYER}"), 1, "Player Layer boots at 1")
        assert_eq(q(tf, f"value.{HEALTH}"), 100, "Player Health boots at 100")
        assert_eq(q(tf, f"value.{SPEED}"), 6.5, "Player Speed boots at 6.5")
        assert_eq(q(tf, "scrubbing"), False, "not scrubbing at boot")

        # ── (B) float scrub right: Speed up over +100px ─────────────
        scrub(tf, numeric_cell(SPEED), 100)
        v = q(tf, f"value.{SPEED}")
        exp = expected_float(6.5, pw, 100)
        assert isinstance(v, float) and abs(v - exp) < 0.05, f"Speed scrubbed to ~{exp:.3f}, got {v}"
        assert v > 6.5, "a rightward drag increases the value"
        assert_eq(q(tf, "scrubbing"), False, "scrub cleared after release")
        assert_eq(q(tf, "editing"), None, "a scrub does not open the inline editor")

        # ── (C) float scrub left: signed — drags back toward 6.5 ────
        scrub(tf, numeric_cell(SPEED), -100)
        v2 = q(tf, f"value.{SPEED}")
        assert v2 < v, "a leftward drag decreases the value"
        assert abs(v2 - 6.5) < 0.05, f"-100px returns Speed to ~6.5, got {v2}"

        # ── (D) int scrub: Health steps in whole units (8px/step) ───
        scrub(tf, numeric_cell(HEALTH), 80)
        steps = expected_int_steps(pw, 80)
        assert_eq(q(tf, f"value.{HEALTH}"), 100 + steps, f"Health steps +{steps} over +80px")
        assert isinstance(q(tf, f"value.{HEALTH}"), int), "an int scrub stays an int"

        # ── (E) int scrub left: steps back down ─────────────────────
        health_hi = q(tf, f"value.{HEALTH}")
        scrub(tf, numeric_cell(HEALTH), -40)
        back = expected_int_steps(pw, 40)
        assert_eq(q(tf, f"value.{HEALTH}"), health_hi - back, f"Health steps -{back} over -40px")

        # ── (F) MULTI-object scrub: Layer shifts EACH, mix preserved ─
        tf.invoke("/external/select_all", None)
        wait_until(lambda: True if q(tf, "selection_count") == 3 else None,
                   desc="all three objects selected")
        # Common rows across all: Visible(0) Layer(1) Locked(2). Layer is 1,1,2.
        assert_eq(q(tf, f"name.{LAYER}"), "Layer", "common row 1 is Layer")
        assert_eq(q(tf, f"mixed.{LAYER}"), True, "Layer is 1,1,2 -> Multiple Values")
        pw2 = panel_width(tf)
        scrub(tf, numeric_cell(LAYER), 24)  # +24px -> +3 steps to EACH
        k = expected_int_steps(pw2, 24)
        assert k >= 1, "the drag crossed at least one step"
        assert_eq(q(tf, f"mixed.{LAYER}"), True, "still mixed after a uniform relative shift")
        assert_eq(q(tf, "scrubbing"), False, "scrub cleared after release")
        # Verify EACH object moved by the same k (the mix-preserving write): re-select
        # each object alone and read its Layer (still common row 1 for all three).
        tf.invoke("/external/select", 0)
        assert_eq(q(tf, f"value.{LAYER}"), 1 + k, f"Player Layer 1 + {k}")
        tf.invoke("/external/select", 1)
        assert_eq(q(tf, f"value.{LAYER}"), 1 + k, f"Camera Layer 1 + {k}")
        tf.invoke("/external/select", 2)
        assert_eq(q(tf, f"value.{LAYER}"), 2 + k, f"Light Layer 2 + {k} (mix preserved)")

        # ── (G) isolation: the Layer scrub touched only Layer ───────
        tf.invoke("/external/select", 0)  # Player
        assert abs(q(tf, f"value.{SPEED}") - 6.5) < 0.05, "Player Speed untouched by the Layer scrub"
        assert_eq(q(tf, f"value.{HEALTH}"), health_hi - back, "Player Health at its (E) value")

        # ── (H) a click (no move) does not scrub and does not edit ──
        before = q(tf, f"value.{SPEED}")
        tf.click(path=numeric_cell(SPEED))
        assert abs(q(tf, f"value.{SPEED}") - before) < 1e-9, "a click leaves the value unchanged"
        assert_eq(q(tf, "scrubbing"), False, "a click never calibrates a scrub")
        assert_eq(q(tf, "editing"), None, "a single click on a numeric cell does not edit")

        # ── (I) a drag on a non-numeric (bool) cell never scrubs ────
        assert_eq(q(tf, "value.0"), True, "Player Visible boots true")
        scrub(tf, f"{INSPECTOR}#toggle0", 70)  # drag the bool cell
        assert_eq(q(tf, "value.0"), False, "the bool toggles on release (the drag did not scrub)")
        assert_eq(q(tf, "scrubbing"), False, "a non-numeric drag never scrubs")

        # ── (J) double-click still opens the inline editor ──────────
        tf.double_click(path=numeric_cell(SPEED))
        wait_until(lambda: q(tf, "editing") == SPEED,
                   desc="a double-click opens the type-in editor on Speed (the edit path is intact)")
        tf.invoke("/external/cancel_edit", None)
        wait_until(lambda: q(tf, "editing") is None, desc="cleanup: editor closed")


if __name__ == "__main__":
    sys.exit(run_demo("R1373 §5.38 — inspector numeric drag-scrub (multi-object)", body))
