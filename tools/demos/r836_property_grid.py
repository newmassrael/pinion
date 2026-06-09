#!/usr/bin/env python3
"""R836 §5.38 §5.40 §5.50 — property-grid / inspector detail panel.

The editor's "Details" panel: a single-tab-stop WAI-ARIA grid of typed
editable rows (bool / int / float / text), each value edited in place by a
type-appropriate control. Built as a pure composition of a
`PropertyGridExternal` coordinator (the typed value model + roving cursor +
edit-mode latch) plus ONE shared `TextFieldExternal` inline editor — the
todomvc edit-in-cell shape generalised to a typed grid.

The whole grid is AI-introspectable (§2 #2): every typed value, name and
kind reads through the primary external, and a value can be set
programmatically without simulating typing (`intervene /external/value.<i>`).

Coordinator slots (`property_grid`, the primary external):
  /external/row_count        -> 9
  /external/focused_row      -> roving cursor row index
  /external/editing          -> null | int (row being text-edited)
  /external/name.<i>         -> property name
  /external/kind.<i>         -> "bool" | "int" | "float" | "text"
  /external/value.<i>        -> the typed value
  /external/toggle           -> invoke: flip the focused bool
  /external/begin            -> invoke(int): enter edit mode on a row
  /external/send             -> invoke: composite "<row>:<Event>" routing

Verified (>= 30 assertions):
  (A) boot taxonomy — 9 rows, the kind quartet, the seed values
  (B) keyboard roving — Down/Up/Home/End move + clamp (a grid has ends)
  (C) bool toggle — Space flips the focused bool; single-click toggles too
  (D) text edit via keyboard — Enter edits, type, Enter commits
  (E) int edit + numeric gate — letters dropped, Enter commits the number
  (F) Escape cancels — the value is left untouched
  (G) programmatic set — intervene writes a typed value strictly
  (H) double-click enters edit mode on an editable row
  (I) paint — rows render; the inline field paints only while editing
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (460, 620)
PAUSE = 0.10

GRID = "property_grid"
EDIT = "property_grid_edit"


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _row_painted(tf, index: int) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, f"{GRID}#{index}") is not None


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert_eq(tf.query("/external/row_count"), 12, "12 property rows")
        assert_eq(tf.query("/external/focused_row"), 0, "cursor boots at row 0")
        assert_eq(tf.query("/external/editing"), None, "no row editing at boot")
        assert_eq(tf.query("/external/name.0"), "Name", "row 0 name")
        assert_eq(tf.query("/external/name.4"), "Layer", "row 4 name")
        assert_eq(tf.query("/external/kind.0"), "text", "Name is text")
        assert_eq(tf.query("/external/kind.2"), "bool", "Visible is bool")
        assert_eq(tf.query("/external/kind.4"), "int", "Layer is int")
        assert_eq(tf.query("/external/kind.6"), "float", "Pos X is float")
        assert_eq(tf.query("/external/value.0"), "Player", "seed Name")
        assert_eq(tf.query("/external/value.2"), True, "seed Visible")
        assert_eq(tf.query("/external/value.4"), 3, "seed Layer")
        assert_eq(tf.query("/external/value.6"), 12.5, "seed Pos X")

        # ── (B) keyboard roving (clamped — a grid has ends) ──────────
        _focus_grid(tf)
        tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query("/external/focused_row") == 1, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> row 1")
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query("/external/focused_row") == 11, timeout=4.0,
                   interval=0.03, desc="End -> last row")
        tf.key(path=GRID, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focused_row"), 11, "ArrowDown at bottom clamps")
        tf.key(path=GRID, name="Home")
        wait_until(lambda: tf.query("/external/focused_row") == 0, timeout=4.0,
                   interval=0.03, desc="Home -> row 0")
        tf.key(path=GRID, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/focused_row"), 0, "ArrowUp at top clamps")

        # ── (C) bool toggle: Space on the focused bool, then click ───
        tf.intervene("/external/focused_row", 2)  # Visible (bool)
        assert_eq(tf.query("/external/value.2"), True, "Visible true before")
        tf.key(path=GRID, name="Space")
        wait_until(lambda: tf.query("/external/value.2") is False, timeout=4.0,
                   interval=0.03, desc="Space toggles the focused bool")
        # Single-click the Locked bool row toggles it (checkbox affordance).
        assert_eq(tf.query("/external/value.3"), False, "Locked false before click")
        tf.click(path=f"{GRID}#3")
        wait_until(lambda: tf.query("/external/value.3") is True, timeout=4.0,
                   interval=0.03, desc="single-click toggles a bool row")
        assert_eq(tf.query("/external/focused_row"), 3, "click also focuses the row")

        # ── (D) text edit via keyboard: Enter -> type -> Enter ───────
        _focus_grid(tf)
        tf.intervene("/external/focused_row", 0)  # Name (text)
        tf.key(path=GRID, name="Enter")  # enter edit mode
        wait_until(lambda: tf.query("/external/editing") == 0, timeout=4.0,
                   interval=0.03, desc="Enter starts editing row 0")
        wait_until(lambda: _row_painted(tf, 0) and find_by_tag(
            tf.snapshot(source="paint", viewport=VIEWPORT), EDIT) is not None,
            timeout=4.0, interval=0.05, desc="inline field painted in the editing row")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="focus moved into the inline field")
        # Clear the seeded "Player" (6 chars) and type a new value.
        for _ in range(6):
            tf.key(path=EDIT, name="Backspace")
        tf.text("Enemy", path=EDIT)
        tf.key(path=EDIT, name="Enter")  # commit
        wait_until(lambda: tf.query("/external/value.0") == "Enemy", timeout=4.0,
                   interval=0.03, desc="Enter commits the typed text")
        assert_eq(tf.query("/external/editing"), None, "edit mode cleared after commit")

        # ── (E) int edit + numeric gate (letters dropped) ───────────
        _focus_grid(tf)
        tf.intervene("/external/focused_row", 4)  # Layer (int) = 3
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == 4, timeout=4.0,
                   interval=0.03, desc="Enter starts editing the int row")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="focus into the int field")
        tf.key(path=EDIT, name="Backspace")  # clear the seeded "3"
        tf.key(path=EDIT, name="2")
        tf.key(path=EDIT, name="x")  # gated out
        tf.key(path=EDIT, name="5")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: tf.query("/external/value.4") == 25, timeout=4.0,
                   interval=0.03, desc="int commits 25 (the 'x' was dropped)")

        # ── (F) Escape cancels — the value is untouched ─────────────
        _focus_grid(tf)
        tf.intervene("/external/focused_row", 1)  # Tag (text) = "hero"
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == 1, timeout=4.0,
                   interval=0.03, desc="editing the Tag row")
        tf.text("ZZZ", path=EDIT)
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: tf.query("/external/editing") is None, timeout=4.0,
                   interval=0.03, desc="Escape exits edit mode")
        assert_eq(tf.query("/external/value.1"), "hero", "Escape leaves the value untouched")

        # ── (G) programmatic typed set (the AI driving path) ─────────
        tf.intervene("/external/value.6", -8.5)  # Pos X (float)
        assert_eq(tf.query("/external/value.6"), -8.5, "intervene sets a float")
        tf.intervene("/external/value.4", 7)
        assert_eq(tf.query("/external/value.4"), 7, "intervene sets an int")
        tf.intervene("/external/value.2", True)
        assert_eq(tf.query("/external/value.2"), True, "intervene sets a bool")

        # ── (H) double-click enters edit mode on an editable row ─────
        tf.double_click(path=f"{GRID}#0")
        wait_until(lambda: tf.query("/external/editing") == 0, timeout=4.0,
                   interval=0.03, desc="double-click starts editing")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: tf.query("/external/editing") is None, timeout=4.0,
                   interval=0.03, desc="back to navigation")

        # ── (I) paint: rows render; field only while editing ─────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for i in range(12):
            assert find_by_tag(snap, f"{GRID}#{i}") is not None, f"row {i} painted"
        assert find_by_tag(snap, EDIT) is None, "no inline field when not editing"


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R836 §5.38 inspector detail panel", body))
