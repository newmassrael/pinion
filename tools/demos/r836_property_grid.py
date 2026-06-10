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
  /external/row_count        -> 12
  /external/editing          -> null | int (source being text-edited)
  /external/name.<source>    -> property name
  /external/kind.<source>    -> "bool" | "int" | "float" | "text" | "choice" | "color"
  /external/value.<source>   -> the typed value
  /external/toggle           -> invoke(int): flip the bool at a source row
  /external/begin            -> invoke(int): enter edit mode on a source row
  /external/send             -> invoke: composite "<source>:<Event>" routing

Group-by proxy slots (`property_grid_cat`, the R871 extra external):
  /external/cursor           -> roving visual-row cursor (null | int)
  /external/source_at.<pos>  -> data-row source at a visual position (null on a header)
  /external/visible_len      -> headers + visible data rows

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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    cursor_to_source,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (460, 820)

GRID = "property_grid"
EDIT = "property_grid_edit"
CAT = "property_grid_cat"  # R871 group-by proxy (collapse set + roving cursor)


def _cursor_source(tf):
    """The stable source index under the roving visual-row cursor, or None."""
    pos = tf.query(f"/{CAT}/external/cursor")
    if pos is None:
        return None
    return tf.query(f"/{CAT}/external/source_at.{pos}")


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
        assert_eq(tf.query(f"/{CAT}/external/cursor"), None, "no cursor at boot")
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

        # ── (B) keyboard roving over the flatten (headers + data) ────
        _focus_grid(tf)
        # From no cursor, ArrowDown lands on visual row 0 (Identity header).
        tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == 0, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 0 (Identity header)")
        last = tf.query(f"/{CAT}/external/visible_len") - 1
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == last, timeout=4.0,
                   interval=0.03, desc="End -> last visual row")
        tf.key(path=GRID, name="ArrowDown")
        # Clamp no-op: the dispatch commits before the response.
        assert_eq(tf.query(f"/{CAT}/external/cursor"), last, "ArrowDown at bottom clamps")
        tf.key(path=GRID, name="Home")
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == 0, timeout=4.0,
                   interval=0.03, desc="Home -> visual row 0")

        # ── (C) bool toggle: Space on the focused bool, then click ───
        cursor_to_source(tf, CAT, 2)  # Visible (bool)
        assert_eq(tf.query("/external/value.2"), True, "Visible true before")
        tf.key(path=GRID, name="Space")
        wait_until(lambda: tf.query("/external/value.2") is False, timeout=4.0,
                   interval=0.03, desc="Space toggles the focused bool")
        # Single-click the Locked bool row toggles it (checkbox affordance).
        assert_eq(tf.query("/external/value.3"), False, "Locked false before click")
        tf.click(path=f"{GRID}#3")
        wait_until(lambda: tf.query("/external/value.3") is True, timeout=4.0,
                   interval=0.03, desc="single-click toggles a bool row")
        assert_eq(_cursor_source(tf), 3, "click also moves the cursor onto the row")

        # ── (D) text edit via keyboard: Enter -> type -> Enter ───────
        _focus_grid(tf)
        cursor_to_source(tf, CAT, 0)  # Name (text)
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
        cursor_to_source(tf, CAT, 4)  # Layer (int) = 3
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
        cursor_to_source(tf, CAT, 1)  # Tag (text) = "hero"
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
