#!/usr/bin/env python3
"""R914 §5.38 §5.27 — data-grid numeric cell scrub (DCC drag-to-edit).

Drives `hello-data-grid` via JSON-RPC. The editable data grid gains the iconic
DCC interaction the property grid already has: press-and-drag a numeric (`Int` /
`Float`) cell to scrub its value (the Blender / Unreal "drag the number field"
gesture). It is the 3rd consumer of the lifted `pinion_core::DragCalibration`
substrate (after the R786 column resize and the R875 property-grid scrub): the
first captured `pointer_move` calibrates (snapshot the cell's value at press +
the cursor anchor), each later move scrubs `base + travel_px · sensitivity`
live, and the release reports whether a drag ran so a scrub suppresses the
trailing click. `capture_normalize` names the grid container (`data_grid`, a
fixed `GRID_VIEWPORT_W`-wide rect), so the cursor-fraction delta recovers true
pixel travel.

The scrub commits through the SAME clamped `set_cell` funnel the AI
`value.<r>.<c>` write uses, so a drag cannot exceed a bound a keyboard / RPC
edit cannot (the R894 symmetry, now extended to the drag).

Witness (§2 #7 scene-as-data): the value model updates as the cursor drags —
observable through `/external/value.<r>.<c>`. The deterministic `scene/drag`
RPC presses on the cell, marches the cursor through N interpolated frames under
the capture lock (firing the same `pointer_move` arc a real mouse does), and
releases. The native winit capture path is already proven end-to-end with live
pixels by r786 (column resize) / r794 (drag-to-move); this round reuses it, so
the deterministic phase is the gate.

The two numeric columns (Count / Scale) sit at / past the right edge of the
370px viewport at rest, so the demo scrolls them into view first (the R896
horizontal scroll), exactly as r837 does for the trailing columns.

  (A) boot — numeric kinds + seeded values; not scrubbing; grid ~basis wide.
  (B) reveal — scroll the Count / Scale columns into view.
  (C) float scrub right — Scale (1.0) drags up by travel_px · 0.01.
  (D) float scrub left — drags back down; direction is signed.
  (E) int scrub — Count steps in whole units (8px / step).
  (F) clamp — a far-right int scrub clamps at the column max (the set_cell gate).
  (G) isolation — a scrub touches only its own cell.
  (H) click (no move) — does not scrub; focuses the cell.
  (I) non-numeric drag — never scrubs (the bool still toggles on release).
  (J) double-click — still opens the inline editor (the edit path is intact).

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
    find_by_tag,
    run_demo,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
EDIT = "data_grid_edit"
H_SCROLL = "data_grid_hscroll"
VIEWPORT = (460, 320)

# The scrub sensitivities + basis mirror the binding consts
# (GRID_VIEWPORT_W = 370, SCRUB_FLOAT_PER_PX = 0.01, SCRUB_INT_PX_PER_STEP = 8).
REF_W = 370.0
FLOAT_PER_PX = 0.01
INT_PX_PER_STEP = 8.0


def gq(tf, slot):
    return tf.query(f"/external/{slot}")


def snap(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def grid_width(tf) -> int:
    rects = abs_rects_of(snap(tf))
    assert GRID in rects, "grid container present in the paint scene"
    return rects[GRID][2]


def cell_on_screen(s, tag) -> bool:
    """The h-scroll clips a cell's on-screen *rect* (the rows are eager), so a
    scrub press needs the rect to overlap the horizontal viewport. Gate on rect
    overlap (a R896.1 ZERO-FLAKE rule), not merely on the tree node."""
    node = find_by_tag(s, H_SCROLL)
    rects = abs_rects_of(s)
    if node is None or tag not in rects:
        return False
    vp = node.get("viewport") or {}
    vp_x, vp_w = int(vp.get("x", -1)), int(vp.get("w", -1))
    x, _, w, _ = rects[tag]
    return x < vp_x + vp_w and x + w > vp_x


def cell_rect(tf, row: int, col: int):
    rects = abs_rects_of(snap(tf))
    tag = f"{GRID}#{row}_{col}"
    assert tag in rects, f"cell {tag} present in the paint scene"
    return rects[tag]


def scrub(tf, row: int, col: int, dx: int) -> None:
    """Press the centre of cell `(row, col)` and drag the captured cursor `dx`
    logical px horizontally (positive = right = increase)."""
    x, y, w, h = cell_rect(tf, row, col)
    cx = x + w // 2
    cy = y + h // 2
    tf.drag(from_at=(float(cx), float(cy)), to_at=(float(cx + dx), float(cy)), steps=10)


def expected_float_delta(gw: int, dx: int) -> float:
    """The value delta a `dx`-pixel cursor drag produces: x_rel moves dx/gw, so
    travel_px = (dx/gw)·REF_W, and the value moves travel_px·FLOAT_PER_PX."""
    return (dx / gw) * REF_W * FLOAT_PER_PX


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: numeric kinds + seeded values ─────────────────
        gw = grid_width(tf)
        assert abs(gw - REF_W) <= 4, f"grid paints at ~{REF_W}px (the scrub basis), got {gw}"
        assert_eq(gq(tf, "row_count"), 4, "4 rows")
        assert_eq(gq(tf, "col_count"), 6, "6 columns")
        assert_eq(gq(tf, "col_kind.2"), "int", "Count (col 2) is an int")
        assert_eq(gq(tf, "col_kind.3"), "float", "Scale (col 3) is a float")
        assert_eq(gq(tf, "col_kind.4"), "bool", "Active (col 4) is a bool")
        assert_eq(gq(tf, "value.0.2"), 1, "Count (0,2) boots at 1")
        assert_eq(gq(tf, "value.0.3"), 1.0, "Scale (0,3) boots at 1.0")
        assert_eq(gq(tf, "scrubbing"), False, "not scrubbing at boot")

        # ── (B) reveal: scroll the Count / Scale columns into view ──
        tf.scroll(H_SCROLL, to=(1000, 0))  # clamps to max => reveals Count/Scale/Active
        wait_snap(tf, lambda s: cell_on_screen(s, f"{GRID}#0_3"),
                  viewport=VIEWPORT, desc="Scale column scrolled on-screen (rect visible)")
        assert cell_on_screen(snap(tf), f"{GRID}#0_2"), "Count column also on-screen"

        # ── (C) float scrub right: Scale up over +60px ──────────────
        scrub(tf, 0, 3, 60)
        v03 = gq(tf, "value.0.3")
        exp = 1.0 + expected_float_delta(gw, 60)
        assert isinstance(v03, float) and abs(v03 - exp) < 0.05, \
            f"Scale scrubbed to ~{exp:.3f}, got {v03}"
        assert v03 > 1.0, "a rightward drag increases the value"
        assert_eq(gq(tf, "scrubbing"), False, "scrub cleared after release")
        assert_eq(gq(tf, "editing_row"), None, "a scrub does not open the inline editor")

        # ── (D) float scrub left: signed — drags back toward 1.0 ────
        scrub(tf, 0, 3, -60)
        v03b = gq(tf, "value.0.3")
        assert v03b < v03, "a leftward drag decreases the value"
        assert abs(v03b - 1.0) < 0.05, f"-60px returns Scale to ~1.0, got {v03b}"

        # ── (E) int scrub: Count steps in whole units (8px/step) ────
        scrub(tf, 0, 2, 64)
        steps = round((64 / gw) * REF_W / INT_PX_PER_STEP)
        assert_eq(gq(tf, "value.0.2"), 1 + steps, f"Count steps +{steps} over +64px")
        assert isinstance(gq(tf, "value.0.2"), int), "an int scrub stays an int"

        # ── (F) clamp: the scrub commits through the clamped set_cell ─
        # funnel — the same gate the AI `value` write runs (R894), so a drag
        # cannot exceed a bound a keyboard / RPC edit cannot. Seed the cell
        # near each bound, then a modest scrub overshoots it.
        tf.intervene("/external/value.0.2", 990)  # near the 0..1000 max
        scrub(tf, 0, 2, 160)  # +160px ~ +20 steps overshoots 1000
        assert_eq(gq(tf, "value.0.2"), 1000, "rightward scrub clamps to the column max")
        tf.intervene("/external/value.0.2", 10)  # near the min
        scrub(tf, 0, 2, -160)  # -160px ~ -20 steps undershoots 0
        assert_eq(gq(tf, "value.0.2"), 0, "leftward scrub clamps to the column min")

        # ── (G) isolation: a scrub touches only its own cell ────────
        assert abs(gq(tf, "value.0.3") - 1.0) < 0.05, "Scale untouched by the Count scrubs"
        assert_eq(gq(tf, "value.1.2"), 24, "Count of row 1 untouched")

        # ── (H) a click on a numeric cell FOCUSES it (R915) ─────────
        # A press within DRAG_CLICK_THRESHOLD_PX is a click, not a scrub: it
        # focuses the cell (value unchanged, no scrub, no edit). Only a drag past
        # the threshold scrubs (sections C-F). R915 fixed the R914 absorption.
        before = gq(tf, "value.1.2")
        tf.click(path=f"{GRID}#1_2")
        # No-op verification: the dispatch commits before the RPC response.
        assert_eq(gq(tf, "value.1.2"), before, "a click leaves the numeric value unchanged")
        assert_eq(gq(tf, "scrubbing"), False, "a click leaves no scrub live")
        assert_eq(gq(tf, "editing_row"), None, "a single click on a numeric cell does not edit")
        assert_eq(gq(tf, "focused_row"), 1, "R915: the click focuses the numeric cell's row")
        assert_eq(gq(tf, "focused_col"), 2, "R915: the click focuses the numeric cell's column")

        # ── (I) a click on a non-numeric (bool) cell still toggles ──
        # The bool column never arms a scrub, so its click falls through to the
        # focus + toggle action (the R837 click path is intact under capture).
        assert_eq(gq(tf, "value.2.4"), False, "Active (2,4) boots false")
        tf.click(path=f"{GRID}#2_4")
        assert_eq(gq(tf, "value.2.4"), True, "the bool toggles on click (never armed a scrub)")
        assert_eq(gq(tf, "focused_row"), 2, "the bool click focuses its cell")
        assert_eq(gq(tf, "scrubbing"), False, "a non-numeric click never scrubs")

        # ── (J) double-click still opens the inline editor ──────────
        tf.double_click(path=f"{GRID}#0_2")
        wait_until(
            lambda: gq(tf, "editing_row") == 0 and gq(tf, "editing_col") == 2,
            desc="double-click opens the editor on (0,2) (the edit path is intact)",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R914 §5.38 — data-grid numeric cell scrub", body))
