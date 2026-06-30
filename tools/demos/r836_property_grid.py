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
  /external/row_count        -> 16 (value-model slots: scalars + struct fields)
  /external/editing          -> null | int (value index being text-edited)
  /external/name.<i>         -> property name (qualified for a struct field)
  /external/kind.<i>         -> "bool" | "int" | "float" | "text" | "choice" | "color"
  /external/value.<i>        -> the typed value
  /external/cursor           -> roving cursor node id (null | leaf "6" | branch cat./struct.)
  /external/toggle           -> invoke(int): flip the bool at a value index
  /external/begin            -> invoke(int): enter edit mode on a value index
  /external/send             -> invoke: composite "<id>:<Event>" routing

Tree-structure introspection (`property_grid_tree`, the R921 read-only extra):
  /external/row_count        -> visible row count (the tree flatten)
  /external/cursor_index     -> the cursor's visual position (null | int)
  /external/id_at.<pos>      -> a row's node id; /external/level_at.<pos> -> aria-level

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
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (460, 820)

GRID = "property_grid"
EDIT = "property_grid_edit"
TREE = "property_grid_tree"  # R921 read-only tree-structure introspection


def _cursor_source(tf):
    """The leaf value index under the roving cursor, or None (the cursor is the
    leaf's node id — its value index in decimal; a branch id is non-numeric)."""
    cur = tf.query("/external/cursor")
    return int(cur) if isinstance(cur, str) and cur.isdigit() else None


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
        assert_eq(tf.query("/external/row_count"), 16, "12 scalars + 4 struct fields")
        assert_eq(tf.query("/external/cursor"), None, "no cursor at boot")
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

        # ── (B) keyboard roving over the flatten (the WAI-ARIA Tree) ─
        # The cursor is an id-keyed node; the tree introspection reports its
        # visual position as `cursor_index` over the `row_count` flatten.
        _focus_grid(tf)
        # From no cursor, ArrowDown lands on visual row 0 (the Identity branch).
        tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == 0, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 0 (Identity category)")
        last = tf.query(f"/{TREE}/external/row_count") - 1
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == last, timeout=4.0,
                   interval=0.03, desc="End -> last visual row")
        tf.key(path=GRID, name="ArrowDown")
        # Clamp no-op: the dispatch commits before the response.
        assert_eq(tf.query(f"/{TREE}/external/cursor_index"), last, "ArrowDown at bottom clamps")
        tf.key(path=GRID, name="Home")
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == 0, timeout=4.0,
                   interval=0.03, desc="Home -> visual row 0")

        # ── (C) bool toggle: Space on the focused bool, then click ───
        tf.intervene("/external/cursor", str(2))  # Visible (bool)
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
        tf.intervene("/external/cursor", str(0))  # Name (text)
        tf.key(path=GRID, name="Enter")  # enter edit mode
        wait_until(lambda: tf.query("/external/editing") == "0", timeout=4.0,
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
        tf.intervene("/external/cursor", str(4))  # Layer (int) = 3
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == "4", timeout=4.0,
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
        # R1176 — slot 1 is now the Mesh asset picker (not inline text), so this
        # inline-edit-and-Escape check uses the Name text leaf (slot 0, already
        # committed to "Enemy" by the edit above — Escape must leave that intact).
        tf.intervene("/external/cursor", str(0))  # Name (text) = "Enemy"
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == "0", timeout=4.0,
                   interval=0.03, desc="editing the Name row")
        tf.text("ZZZ", path=EDIT)
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: tf.query("/external/editing") is None, timeout=4.0,
                   interval=0.03, desc="Escape exits edit mode")
        assert_eq(tf.query("/external/value.0"), "Enemy", "Escape leaves the value untouched")

        # ── (G) programmatic typed set (the AI driving path) ─────────
        tf.intervene("/external/value.6", -8.5)  # Pos X (float)
        assert_eq(tf.query("/external/value.6"), -8.5, "intervene sets a float")
        tf.intervene("/external/value.4", 7)
        assert_eq(tf.query("/external/value.4"), 7, "intervene sets an int")
        tf.intervene("/external/value.2", True)
        assert_eq(tf.query("/external/value.2"), True, "intervene sets a bool")

        # ── (H) double-click enters edit mode on an editable row ─────
        tf.double_click(path=f"{GRID}#0")
        wait_until(lambda: tf.query("/external/editing") == "0", timeout=4.0,
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
