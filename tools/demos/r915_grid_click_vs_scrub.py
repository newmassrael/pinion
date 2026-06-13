#!/usr/bin/env python3
"""R915 §5.27 §5.35 — data-grid click-to-focus vs drag-to-scrub threshold split.

Drives `hello-data-grid` via JSON-RPC. R914 added the numeric cell scrub but, by
calibrating a zero-travel scrub on the R51.35 press-time forward, *absorbed* a
plain click on a numeric cell (it neither scrubbed nor focused). R915 completes
the gesture the canonical DCC way (Blender / Unreal / Excel): a press whose
cursor stays within `DRAG_CLICK_THRESHOLD_PX` is a **click** (focuses the cell);
only a press that strays past the threshold becomes a **scrub** (which suppresses
the focus-click). The discriminator is the shared `DragCalibration::traveled_beyond`
gate — one decision behind the scrub mutation, the click suppression, and the
AI-first `scrubbing` query.

Witness (§2 #7 scene-as-data): the focused cell + the cell value are both
observable through `/external/{focused_row,focused_col,value.<r>.<c>,scrubbing}`.
The deterministic `scene/click` / `scene/drag` RPCs inject the same press / move /
release arc a real mouse fires under the capture lock.

  (A) boot + reveal the numeric Count / Scale columns.
  (B) click a numeric cell — focuses it; value unchanged; not scrubbing.
  (C) click a different numeric cell — focus follows the click.
  (D) dead-zone drag (< threshold) — still a click: focuses, does not scrub.
  (E) real drag (> threshold) — scrubs the value.
  (F) a scrub suppresses the focus-click — focus stays on the prior cell.
  (G) a click on a bool cell still toggles + focuses (non-numeric path intact).

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
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
H_SCROLL = "data_grid_hscroll"
VIEWPORT = (460, 320)


def gq(tf, slot):
    return tf.query(f"/external/{slot}")


def snap(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def cell_on_screen(s, tag) -> bool:
    node = find_by_tag(s, H_SCROLL)
    rects = abs_rects_of(s)
    if node is None or tag not in rects:
        return False
    vp = node.get("viewport") or {}
    vp_x, vp_w = int(vp.get("x", -1)), int(vp.get("w", -1))
    x, _, w, _ = rects[tag]
    return x < vp_x + vp_w and x + w > vp_x


def cell_center(tf, row: int, col: int):
    rects = abs_rects_of(snap(tf))
    tag = f"{GRID}#{row}_{col}"
    assert tag in rects, f"cell {tag} present in the paint scene"
    x, y, w, h = rects[tag]
    return float(x + w // 2), float(y + h // 2)


def drag_px(tf, row: int, col: int, dx: int) -> None:
    """Press the centre of cell (row, col) and drag the captured cursor dx px."""
    cx, cy = cell_center(tf, row, col)
    tf.drag(from_at=(cx, cy), to_at=(cx + dx, cy), steps=6)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot + reveal ────────────────────────────────────────
        assert_eq(gq(tf, "row_count"), 4, "4 rows")
        assert_eq(gq(tf, "col_count"), 5, "5 columns")
        assert_eq(gq(tf, "col_kind.2"), "int", "Count (col 2) is an int")
        assert_eq(gq(tf, "col_kind.3"), "float", "Scale (col 3) is a float")
        assert_eq(gq(tf, "col_kind.4"), "bool", "Active (col 4) is a bool")
        assert_eq(gq(tf, "value.0.2"), 1, "Count (0,2) boots at 1")
        assert_eq(gq(tf, "value.0.3"), 1.0, "Scale (0,3) boots at 1.0")
        assert_eq(gq(tf, "scrubbing"), False, "not scrubbing at boot")
        tf.scroll(H_SCROLL, to=(1000, 0))  # reveal Count / Scale / Active
        wait_snap(tf, lambda s: cell_on_screen(s, f"{GRID}#0_3"),
                  viewport=VIEWPORT, desc="Scale column scrolled on-screen")
        assert cell_on_screen(snap(tf), f"{GRID}#0_2"), "Count column also on-screen"

        # ── (B) a click on a numeric cell FOCUSES it ─────────────────
        tf.click(path=f"{GRID}#0_2")
        assert_eq(gq(tf, "focused_row"), 0, "click focuses the cell row")
        assert_eq(gq(tf, "focused_col"), 2, "click focuses the cell column")
        assert_eq(gq(tf, "value.0.2"), 1, "the click did not change the Count value")
        assert_eq(gq(tf, "scrubbing"), False, "a click leaves no scrub live")
        assert_eq(gq(tf, "editing_row"), None, "a single click does not edit")

        # ── (C) clicking a different numeric cell moves the focus ────
        tf.click(path=f"{GRID}#2_3")
        assert_eq(gq(tf, "focused_row"), 2, "focus follows the click to row 2")
        assert_eq(gq(tf, "focused_col"), 3, "focus follows the click to col 3")
        assert_eq(gq(tf, "value.2.3"), 0.5, "the click did not change the Scale value")

        # ── (D) a dead-zone drag (< threshold) is still a click ──────
        # 2px stray < DRAG_CLICK_THRESHOLD_PX (4px): focuses, never scrubs.
        before_03 = gq(tf, "value.0.3")
        drag_px(tf, 0, 3, 2)
        assert_eq(gq(tf, "value.0.3"), before_03, "a 2px stray stays within the click dead zone")
        assert_eq(gq(tf, "scrubbing"), False, "the dead-zone press never scrubs")
        assert_eq(gq(tf, "focused_row"), 0, "the dead-zone click focuses row 0")
        assert_eq(gq(tf, "focused_col"), 3, "the dead-zone click focuses col 3")

        # ── (E) a real drag (> threshold) scrubs the value ───────────
        drag_px(tf, 0, 3, 60)
        v03 = gq(tf, "value.0.3")
        assert isinstance(v03, float) and v03 > before_03, \
            f"a 60px rightward drag scrubs Scale up (was {before_03}, now {v03})"
        assert_eq(gq(tf, "scrubbing"), False, "scrub cleared after release")

        # ── (F) a scrub SUPPRESSES the focus-click ───────────────────
        # Focus a known cell, then scrub a DIFFERENT cell: the scrub must not
        # move the focus onto the scrubbed cell (the click is suppressed).
        tf.click(path=f"{GRID}#0_2")
        assert_eq(gq(tf, "focused_row"), 0, "focus parked on (0,2)")
        assert_eq(gq(tf, "focused_col"), 2, "focus parked on (0,2)")
        before_12 = gq(tf, "value.1.2")
        drag_px(tf, 1, 2, 80)  # real drag-scrub on (1,2)
        assert gq(tf, "value.1.2") != before_12, "the drag scrubbed Count of row 1"
        assert_eq(gq(tf, "focused_row"), 0, "the scrub did NOT move focus to the scrubbed row")
        assert_eq(gq(tf, "focused_col"), 2, "the scrub did NOT move focus to the scrubbed col")

        # ── (G) a click on a bool cell toggles + focuses ─────────────
        assert_eq(gq(tf, "value.2.4"), False, "Active (2,4) boots false")
        tf.click(path=f"{GRID}#2_4")
        assert_eq(gq(tf, "value.2.4"), True, "the bool toggles on click")
        assert_eq(gq(tf, "focused_row"), 2, "the bool click focuses its row")
        assert_eq(gq(tf, "focused_col"), 4, "the bool click focuses its col")
        assert_eq(gq(tf, "scrubbing"), False, "a bool click never scrubs")


if __name__ == "__main__":
    sys.exit(run_demo("R915 §5.27 — data-grid click-focus vs drag-scrub split", body))
