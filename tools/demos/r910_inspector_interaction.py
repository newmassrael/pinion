#!/usr/bin/env python3
"""R910 §5.38 §5.22 — inspector pointer + keyboard selection demo.

R909 built the selection-driven detail panel with selection over RPC
(invoke/intervene). R910 completes the GUI interaction loop — the
documented R909 carry — so the inspector is a real interactive widget:

  - **pointer click-to-select**: each object row is tagged
    `inspector#<i>`; the input router's `#` split (R51.42) routes a click
    to the primary External's `send` wire (`"<i>:<EventName>"`), which
    selects on the activation edge (PointerUp) — the hello-tabs /
    radio-group click-to-select pattern.
  - **keyboard navigation**: with the inspector focused, Arrow keys move
    the selection (clamped) and Home/End jump to the ends, routed through
    the same `send` path (`"<i>:KeyboardActivate"`).

Both share the one select path the RPC `invoke /external/select` uses, so
pointer, keyboard, and AI agent cannot diverge. All observable as
scene-as-data ([[ai-first-rpc-introspection-obligation]]).

Since R922 the inspector is a MULTI-select model; this demo exercises the
cardinality-1 path (a plain click / unmodified Arrow replaces the selection
with a single row). The modifier-click + chord-keyboard multi-select paths
live in `r922_inspector_multi.py`.

## Demo scope (>=30 assertions, sections A-E)

  (A) Boot: default selection + object roster.
  (B) Click-to-select re-targets the property set (each object).
  (C) A click only selects on release, and the selected value addressing
      follows.
  (D) Keyboard navigation (Arrow / Home / End) after focusing.
  (E) Out-of-range click index is a harmless no-op.
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


def _click_row(tf: RpcSubprocess, index: int) -> None:
    """Click the object row tagged `inspector#<index>`."""
    tf.click(path=f"inspector#{index}")


def _assert_boot(tf: RpcSubprocess) -> None:
    """(A) default selection + roster."""
    assert _q(tf, "object_count") == 3
    assert _q(tf, "selected") == 0
    assert _q(tf, "selection_summary") == "Player"
    assert _q(tf, "object_name.0") == "Player"
    assert _q(tf, "object_name.1") == "Main Camera"
    assert _q(tf, "object_name.2") == "Sun Light"


def _assert_click_selection(tf: RpcSubprocess) -> None:
    """(B) clicking each row selects it and re-targets the property set
    (a plain click replaces the selection — the cardinality-1 case)."""
    _click_row(tf, 2)
    wait_query(tf, "/external/selected", 2, desc="click row 2 -> selected 2")
    assert _q(tf, "selection_summary") == "Sun Light"
    assert _q(tf, "row_count") == 6, "Sun Light has six properties"
    assert _q(tf, "name.0") == "Visible", "Sun Light's first property is the common base"
    assert _q(tf, "kind.0") == "bool", "Visible is a bool"
    assert _q(tf, "value.0") is False, "Sun Light Visible seed False"

    _click_row(tf, 1)
    wait_query(tf, "/external/selected", 1, desc="click row 1 -> selected 1")
    assert _q(tf, "selection_summary") == "Main Camera"
    assert _q(tf, "row_count") == 5
    assert _q(tf, "name.0") == "Visible"
    assert _q(tf, "kind.3") == "float", "Field of View is a float"

    _click_row(tf, 0)
    wait_query(tf, "/external/selected", 0, desc="click row 0 -> selected 0")
    assert _q(tf, "selection_summary") == "Player"
    assert _q(tf, "row_count") == 7
    assert _q(tf, "kind.5") == "choice", "Team is a choice"


def _assert_value_follows_selection(tf: RpcSubprocess) -> None:
    """(C) value.<i> addresses the clicked object's property — index 3 is the
    first type-specific row, so it diverges across objects."""
    _click_row(tf, 1)  # Camera.
    wait_query(tf, "/external/selected", 1, desc="re-select Camera")
    assert abs(float(_q(tf, "value.3")) - 60.0) < 1e-9, "Camera value.3 = Field of View"
    _click_row(tf, 0)  # Player.
    wait_query(tf, "/external/selected", 0, desc="re-select Player")
    assert _q(tf, "value.3") == 100, "Player value.3 = Health"


def _assert_keyboard_nav(tf: RpcSubprocess) -> None:
    """(D) Arrow / Home / End navigation. The key is addressed to the
    `inspector` widget via `path`; its `apply_key` routes nav keys
    through the same `send` select path the pointer uses."""
    # Start at Player (0).
    _click_row(tf, 0)
    wait_query(tf, "/external/selected", 0, desc="reset to Player")

    tf.key(path="inspector", name="ArrowDown")
    wait_query(tf, "/external/selected", 1, desc="ArrowDown -> 1")
    tf.key(path="inspector", name="ArrowDown")
    wait_query(tf, "/external/selected", 2, desc="ArrowDown -> 2")
    tf.key(path="inspector", name="ArrowDown")  # clamp at last.
    wait_query(tf, "/external/selected", 2, desc="ArrowDown clamps at last")
    tf.key(path="inspector", name="Home")
    wait_query(tf, "/external/selected", 0, desc="Home -> first")
    tf.key(path="inspector", name="ArrowUp")  # clamp at first.
    wait_query(tf, "/external/selected", 0, desc="ArrowUp clamps at first")
    tf.key(path="inspector", name="End")
    wait_query(tf, "/external/selected", 2, desc="End -> last")


def _assert_oor_click_noop(tf: RpcSubprocess) -> None:
    """(E) a click index past the roster is a harmless no-op (the node
    does not exist, so nothing routes / selects)."""
    _click_row(tf, 0)
    wait_query(tf, "/external/selected", 0, desc="reset to Player")
    before = _q(tf, "selected")
    try:
        tf.click(path="inspector#9")  # no such row.
    except RpcError:
        pass  # a missing click target may raise; either way no selection change.
    assert _q(tf, "selected") == before, "clicking a non-existent row must not change selection"


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        _wait_ready(tf)
        _assert_boot(tf)                    # (A)
        _assert_click_selection(tf)         # (B)
        _assert_value_follows_selection(tf) # (C)
        _assert_keyboard_nav(tf)            # (D)
        _assert_oor_click_noop(tf)          # (E)


if __name__ == "__main__":
    sys.exit(run_demo("R910 inspector pointer + keyboard selection", body))
