#!/usr/bin/env python3
"""R922 §5.38 §5.40 — multi-object inspector (the engine Details core).

R909/R910 built the single-object selection-driven Details panel. R922
extends it to its multi-object core: select several objects at once and the
panel shows the properties **common** to every selected object, a "Multiple
Values" placeholder wherever they differ, and an edit that writes the new
value into **all** selected objects at once.

The selection is the pure `VirtualSelect` index-set model (R780), embedded
directly in the inspector coordinator; the chord policy (`SelectionChord` /
`MultiSelectKeyOp`) and the NaN-safe value equality (`CellValue::value_eq`)
are reused, not re-implemented. Keyboard, modifier-click, and RPC all
converge on one select / toggle / extend funnel.

## The verification idea: scene-as-data, deterministic

Everything is introspectable over RPC (§2 #2 AI-first primary path), so no
pixels. The three objects share a common actor base (Visible / Layer /
Locked) plus type-specific tails; the base values are a deliberate mix so a
multi-selection surfaces both uniform and "Multiple Values" rows.

## Demo scope (>=30 assertions, sections A-H)

  (A) Boot: the multi-select wire (mode / selection / selection_count) and
      the cardinality-1 panel.
  (B) The RPC selection funnel: toggle builds a set, the panel becomes the
      common properties, clear empties it.
  (C) select_all reports "Multiple Values" on the differing base properties.
  (D) intervene 'selection' restores an arbitrary set; the mixed flags follow.
  (E) the multi-object edit: writing a common property hits every selected
      object and resolves the mixed state.
  (F) modifier pointer-clicks (the keyboard chords' pointer peer).
  (G) keyboard chords: Shift+Arrow extend, Ctrl+Space toggle, Ctrl+A all.
  (H) errors: read-only derived axes, out-of-range toggle.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    runs_of,
    wait_query,
    wait_until,
)


def _q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def _wait_ready(tf: RpcSubprocess) -> None:
    wait_until(
        lambda: True if _q(tf, "object_count") == 3 else None,
        desc="inspector external ready",
    )


def _assert_boot_multi_wire(tf: RpcSubprocess) -> None:
    """(A) the multi-select wire + the cardinality-1 panel."""
    assert _q(tf, "object_count") == 3
    assert _q(tf, "mode") == "multi", "the inspector list is a multi-select model"
    assert _q(tf, "selection") == runs_of([0]), "boots with the first object selected"
    assert _q(tf, "selection_count") == 1
    assert _q(tf, "selected") == 0, "the cursor is the lone selected object"
    assert _q(tf, "selection_summary") == "Player"
    assert _q(tf, "row_count") == 7, "a single selection shows that object's full schema"


def _assert_funnel_builds_common_panel(tf: RpcSubprocess) -> None:
    """(B) toggle builds a multi-selection; the panel becomes the common
    properties; clear empties it."""
    assert tf.invoke("/external/toggle", 1) == runs_of([0, 1]), "toggle returns the new set"
    wait_query(tf, "/external/selection", runs_of([0, 1]), desc="Player + Camera selected")
    assert _q(tf, "selection_count") == 2
    assert _q(tf, "selection_summary") == "2 objects selected"
    # The panel is now the common actor base only (the type-specific tails are
    # not shared between Player and Camera).
    assert _q(tf, "row_count") == 3, "Visible / Layer / Locked are the common base"
    assert _q(tf, "name.0") == "Visible"
    assert _q(tf, "name.1") == "Layer"
    assert _q(tf, "name.2") == "Locked"
    # Player + Camera agree on every base value (both Visible True, Layer 1).
    assert _q(tf, "mixed.0") is False, "Visible agrees across Player + Camera"
    assert _q(tf, "mixed.1") is False, "Layer agrees (both 1)"
    assert _q(tf, "value.1") == 1, "the agreed Layer value is shown"

    tf.invoke("/external/clear", None)
    wait_query(tf, "/external/selection", [], desc="clear empties the selection")
    assert _q(tf, "selected") is None, "no cursor when nothing is selected"
    assert _q(tf, "row_count") == 0, "an empty selection has no common properties"
    assert _q(tf, "selection_summary") == "No selection"


def _assert_select_all_reports_mixed(tf: RpcSubprocess) -> None:
    """(C) select_all → the differing base properties report "Multiple Values"."""
    assert tf.invoke("/external/select_all", None) == runs_of([0, 1, 2])
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="every object selected")
    assert _q(tf, "selection_summary") == "3 objects selected"
    # Visible is (True, True, False); Layer is (1, 1, 2); Locked is
    # (False, False, True) — all three base properties differ across the trio.
    assert _q(tf, "mixed.0") is True, "Visible is mixed across all three"
    assert _q(tf, "mixed.1") is True, "Layer is mixed (1, 1, 2)"
    assert _q(tf, "mixed.2") is True, "Locked is mixed"
    # value.<i> reports the representative (first selected = Player) value.
    assert _q(tf, "value.1") == 1, "value.1 is the representative (Player) Layer"


def _assert_selection_restore_and_mixed(tf: RpcSubprocess) -> None:
    """(D) intervene 'selection' restores an arbitrary set; mixed flags follow."""
    tf.intervene("/external/selection", [[0, 0], [2, 2]])  # Player + Sun Light.
    wait_query(tf, "/external/selection", runs_of([0, 2]), desc="restore the set {Player, Light}")
    assert _q(tf, "selection_count") == 2
    assert _q(tf, "row_count") == 3
    # Player vs Light: Visible (True vs False), Layer (1 vs 2), Locked (False vs True).
    assert _q(tf, "mixed.0") is True, "Visible differs Player vs Light"
    assert _q(tf, "mixed.1") is True, "Layer differs (1 vs 2)"
    assert _q(tf, "mixed.2") is True, "Locked differs"


def _assert_multi_object_edit(tf: RpcSubprocess) -> None:
    """(E) writing a common property hits every selected object and resolves
    the mixed state — the multi-object edit headline."""
    tf.invoke("/external/select_all", None)
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="select every object to edit")
    # Set the common Layer (index 1) to 5 across all three objects at once.
    tf.intervene("/external/value.1", 5)
    wait_query(tf, "/external/value.1", 5, desc="Layer written to 5")
    assert _q(tf, "mixed.1") is False, "after the write the selection agrees"
    # Each object individually now carries Layer == 5.
    for obj in range(3):
        tf.invoke("/external/select", obj)
        wait_query(tf, "/external/selected", obj, desc=f"inspect object {obj}")
        assert _q(tf, "value.1") == 5, f"object {obj} Layer written through to 5"

    # Resolve a mixed bool too: set Visible True across all three.
    tf.invoke("/external/select_all", None)
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="re-select all")
    assert _q(tf, "mixed.0") is True, "Visible still mixed before the write"
    tf.intervene("/external/value.0", True)
    wait_query(tf, "/external/mixed.0", False, desc="Visible now agrees")
    assert _q(tf, "value.0") is True
    for obj in range(3):
        tf.invoke("/external/select", obj)
        wait_query(tf, "/external/selected", obj, desc=f"re-inspect object {obj}")
        assert _q(tf, "value.0") is True, f"object {obj} Visible written through to True"


def _assert_modifier_clicks(tf: RpcSubprocess) -> None:
    """(F) modifier pointer-clicks — the keyboard chords' pointer peer. The
    held modifiers ride the composite `inspector#<i>` click wire
    (dispatch_send_mods → SelectionChord): plain replaces, Ctrl toggles,
    Shift extends from the anchor."""
    tf.click(path="inspector#0")
    wait_query(tf, "/external/selection", runs_of([0]), desc="plain click replaces + anchors at 0")
    tf.modifiers(shift=True)
    tf.click(path="inspector#2")
    tf.modifiers()
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="Shift-click extends 0->2")
    tf.modifiers(ctrl=True)
    tf.click(path="inspector#1")
    tf.modifiers()
    wait_query(tf, "/external/selection", runs_of([0, 2]), desc="Ctrl-click toggles 1 out")


def _assert_keyboard_chords(tf: RpcSubprocess) -> None:
    """(G) keyboard chords routed through the inspector's apply_key — the same
    select / toggle / extend funnel the pointer + RPC drive."""
    tf.request("focus/set", {"tag": "inspector"})
    tf.invoke("/external/select", 0)
    wait_query(tf, "/external/selection", runs_of([0]), desc="reset to {0}, cursor 0")

    # Shift+ArrowDown extends the range from the anchor (0) to the next row.
    tf.modifiers(shift=True)
    tf.key(path="inspector", name="ArrowDown")
    tf.modifiers()
    wait_query(tf, "/external/selection", runs_of([0, 1]), desc="Shift+Down extends 0->1")

    # Ctrl+A selects every row.
    tf.modifiers(ctrl=True)
    tf.key(path="inspector", name="a")
    tf.modifiers()
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="Ctrl+A selects all")

    # Ctrl+Space toggles the active (cursor) row out. The cursor is 1 (the
    # Shift+Down landing row), so row 1 leaves the set.
    tf.modifiers(ctrl=True)
    tf.key(path="inspector", name="Space")
    tf.modifiers()
    wait_query(tf, "/external/selection", runs_of([0, 2]), desc="Ctrl+Space toggles the cursor row off")

    # Plain ArrowDown replaces the whole set with the navigated row (the cursor
    # was 1, clamp_nav -> 2).
    tf.key(path="inspector", name="ArrowDown")
    wait_query(tf, "/external/selection", runs_of([2]), desc="plain nav replaces (selection-follows-focus)")


def _assert_errors(tf: RpcSubprocess) -> None:
    """(H) the derived axes are read-only; an out-of-range toggle is rejected."""
    tf.invoke("/external/select_all", None)
    wait_query(tf, "/external/selection", runs_of([0, 1, 2]), desc="select all for the read-only checks")

    for path, value in (("mixed.0", True), ("name.0", "x"), ("selection_summary", "x")):
        raised = False
        try:
            tf.intervene(f"/external/{path}", value)
        except RpcError:
            raised = True
        assert raised, f"{path} must be read-only"

    # Out-of-range toggle rejected, selection unchanged.
    before = _q(tf, "selection")
    raised = False
    try:
        tf.invoke("/external/toggle", 9)
    except RpcError:
        raised = True
    assert raised, "toggle(9) must be rejected"
    assert _q(tf, "selection") == before, "a rejected toggle leaves the selection unchanged"


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        _wait_ready(tf)
        _assert_boot_multi_wire(tf)              # (A)
        _assert_funnel_builds_common_panel(tf)   # (B)
        _assert_select_all_reports_mixed(tf)     # (C)
        _assert_selection_restore_and_mixed(tf)  # (D)
        _assert_multi_object_edit(tf)            # (E)
        _assert_modifier_clicks(tf)              # (F)
        _assert_keyboard_chords(tf)              # (G)
        _assert_errors(tf)                       # (H)


if __name__ == "__main__":
    sys.exit(run_demo("R922 multi-object inspector", body))
