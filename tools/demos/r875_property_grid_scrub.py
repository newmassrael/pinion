#!/usr/bin/env python3
"""R875 §5.38 §5.27 — property-grid numeric scrub (DCC drag-to-edit).

Drives `hello-property-grid` via JSON-RPC. The Details-panel inspector gains the
iconic DCC interaction: press-and-drag a numeric (`Int` / `Float`) property row
to scrub its value (the Blender / Unreal "drag the number field" gesture). It
reuses the R51.34 pointer-capture lock + the R786 stable-pixel-basis idiom:
`capture_normalize` names the grid container (`property_grid`, a fixed-width
rect), so the cursor-fraction delta recovers true pixel travel; `pointer_move`
writes `base + travel_px · sensitivity` live to the same value model the inline
editor and the RPC `value.<i>` write — one source of truth.

Witness (§2 #7 scene-as-data): the value model the view reads updates as the
cursor drags — observable through `/property_grid/external/value.<i>`. The
deterministic `scene/drag` RPC presses on the row, marches the cursor through N
interpolated frames under the capture lock (firing the same `pointer_move` arc a
real mouse does), and releases. The capture substrate's native winit path is
already proven end-to-end with live pixels by r786 (column resize) / r794
(drag-to-move); this round reuses it, so the deterministic phase is the gate.

  (A) boot — numeric kinds + seeded values; not scrubbing.
  (B) float scrub right — Pos X (12.5) drags up by travel_px · 0.01.
  (C) float scrub left — drags back down; direction is signed.
  (D) int scrub — Layer steps in whole units (8px / step).
  (E) int scrub left — steps back down.
  (F) isolation — a scrub touches only its own row.
  (G) click (no move) — does not scrub and does not edit.
  (H) non-numeric drag — never scrubs (the bool still toggles on release).
  (I) double-click — still opens the inline editor (the edit path is intact).

>= 30 assertions.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-property-grid"
GRID = "property_grid"
VIEWPORT = (460, 820)
PAUSE = 0.10

# The scrub sensitivities mirror the binding consts (GRID_W_PX = 400,
# SCRUB_FLOAT_PER_PX = 0.01, SCRUB_INT_PX_PER_STEP = 8).
REF_W = 400.0
FLOAT_PER_PX = 0.01
INT_PX_PER_STEP = 8.0


def gq(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def snap_rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def grid_width(tf) -> int:
    rects = snap_rects(tf)
    assert GRID in rects, "grid container present"
    return rects[GRID][2]


def row_rect(tf, source: int):
    rects = snap_rects(tf)
    tag = f"{GRID}#{source}"
    assert tag in rects, f"row {tag} present in the paint scene"
    return rects[tag]


def scrub(tf, source: int, dx: int) -> None:
    """Press the numeric row `source` and drag the captured cursor `dx` logical
    px horizontally (positive = right = increase)."""
    x, y, w, h = row_rect(tf, source)
    # Press in the value column (right 60%); the whole row is the scrub target.
    cx = x + int(w * 0.6)
    cy = y + h // 2
    tf.drag(from_at=(float(cx), float(cy)), to_at=(float(cx + dx), float(cy)), steps=10)
    time.sleep(PAUSE)


def expected_float_delta(gw: int, dx: int) -> float:
    """The value delta a `dx`-pixel cursor drag produces: x_rel moves dx/gw, so
    travel_px = (dx/gw)·REF_W, and the value moves travel_px·FLOAT_PER_PX."""
    return (dx / gw) * REF_W * FLOAT_PER_PX


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: numeric kinds + seeded values ─────────────────
        gw = grid_width(tf)
        assert abs(gw - REF_W) <= 4, f"grid paints at ~{REF_W}px (the scrub basis), got {gw}"
        assert_eq(gq(tf, "kind.4"), "int", "Layer (source 4) is an int")
        assert_eq(gq(tf, "kind.5"), "int", "Health (source 5) is an int")
        assert_eq(gq(tf, "kind.6"), "float", "Pos X (source 6) is a float")
        assert_eq(gq(tf, "kind.2"), "bool", "Visible (source 2) is a bool")
        assert_eq(gq(tf, "value.4"), 3, "Layer boots at 3")
        assert_eq(gq(tf, "value.5"), 100, "Health boots at 100")
        assert_eq(gq(tf, "value.6"), 12.5, "Pos X boots at 12.5")
        assert_eq(gq(tf, "scrubbing"), False, "not scrubbing at boot")

        # ── (B) float scrub right: Pos X up by ~+1.0 over +100px ─────
        scrub(tf, 6, 100)
        v6 = gq(tf, "value.6")
        exp = 12.5 + expected_float_delta(gw, 100)
        assert isinstance(v6, float) and abs(v6 - exp) < 0.05, \
            f"Pos X scrubbed to ~{exp:.3f}, got {v6}"
        assert v6 > 12.5, "a rightward drag increases the value"
        assert_eq(gq(tf, "scrubbing"), False, "scrub cleared after release")
        assert_eq(gq(tf, "editing"), None, "a scrub does not open the inline editor")

        # ── (C) float scrub left: signed — drags back toward 12.5 ───
        scrub(tf, 6, -100)
        v6b = gq(tf, "value.6")
        assert v6b < v6, "a leftward drag decreases the value"
        assert abs(v6b - 12.5) < 0.05, f"-100px returns Pos X to ~12.5, got {v6b}"

        # ── (D) int scrub: Layer steps in whole units (8px/step) ────
        scrub(tf, 4, 80)
        steps = round((80 / gw) * REF_W / INT_PX_PER_STEP)
        assert_eq(gq(tf, "value.4"), 3 + steps, f"Layer steps +{steps} over +80px")
        assert isinstance(gq(tf, "value.4"), int), "an int scrub stays an int"

        # ── (E) int scrub left: steps back down ─────────────────────
        layer_hi = gq(tf, "value.4")
        scrub(tf, 4, -40)
        back = round((40 / gw) * REF_W / INT_PX_PER_STEP)
        assert_eq(gq(tf, "value.4"), layer_hi - back, f"Layer steps -{back} over -40px")

        # ── (F) isolation: a scrub touches only its own row ─────────
        assert_eq(gq(tf, "value.5"), 100, "Health untouched by the Layer / Pos X scrubs")
        assert abs(gq(tf, "value.6") - 12.5) < 0.05, "Pos X back at its (C) value"

        # ── (G) a click (no move) does not scrub and does not edit ──
        before = gq(tf, "value.5")
        tf.click(path=f"{GRID}#5")
        time.sleep(PAUSE)
        assert_eq(gq(tf, "value.5"), before, "a click leaves the value unchanged")
        assert_eq(gq(tf, "scrubbing"), False, "a click never calibrates a scrub")
        assert_eq(gq(tf, "editing"), None, "a single click on a numeric row does not edit")

        # ── (H) a drag on a non-numeric row never scrubs ────────────
        assert_eq(gq(tf, "value.2"), True, "Visible boots true")
        scrub(tf, 2, 70)  # drag the bool row
        assert_eq(gq(tf, "value.2"), False, "the bool toggles on release (the drag did not scrub)")
        assert_eq(gq(tf, "scrubbing"), False, "a non-numeric drag never scrubs")

        # ── (I) double-click still opens the inline editor ──────────
        tf.double_click(path=f"{GRID}#5")
        time.sleep(PAUSE)
        assert_eq(gq(tf, "editing"), 5, "double-click opens the editor on Health (the edit path is intact)")


if __name__ == "__main__":
    sys.exit(run_demo("R875 §5.38 — property-grid numeric scrub", body))
