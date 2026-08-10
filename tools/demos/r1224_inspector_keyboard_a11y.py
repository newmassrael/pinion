#!/usr/bin/env python3
"""R1224 §5.39 §5.40 §5.22 — inspector interactive-cell KEYBOARD + focus a11y.

R1221 made the Details value cells interactive (a Bool toggles, a numeric steps)
but ONLY over the mouse (`inspector#toggle<i>` / `inc<i>` / `dec<i>`) and the
RPC verbs — there was no keyboard path to the cells and no interactive a11y role
(the panel was a read-only `list` of `listitem`s). R1224 closes that:

  * the inspector is one focus stop hosting TWO composite panes (the object
    `listbox` + the Details property form), so a `focus_region` axis tracks
    which the Arrow keys drive; **Tab** cycles it. Both the region and the
    active Details **property cursor** are observable + drivable over RPC
    (§2 #2): `focus_region`, `prop_cursor`, `focus_property`.
  * in the Details pane the keyboard EDITS the cursor row — Space/Enter toggles
    a Bool, ArrowLeft/- and ArrowRight/+ step a numeric, Delete/Backspace resets
    a modified row — each through the SAME verb its click / RPC twin uses.
  * the a11y (unit-tested in the crate) makes each interactive row its operable
    role (Bool `switch`, numeric `spinbutton`) with the active-descendant on the
    property cursor — verified here through its RPC-observable peer state.

  (A) boot: the object pane owns the keyboard, no property cursor yet.
  (B) keyboard: Tab cycles the region; Arrow keys move the property cursor and
      Space / +/- / Delete edit the cell at it (all observable, no pixels).
  (C) RPC focus verbs: focus_region / focus_property place + clamp the cursor.
  (D) rejects: an unknown region token / a non-string arg are typed errors.

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r1224_inspector_keyboard_a11y.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

INSPECTOR = "inspector"


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def wait_q(tf: RpcSubprocess, key: str, expected: Any, desc: str) -> None:
    wait_until(lambda: True if q(tf, key) == expected else None, desc=desc)


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        # ── (A) boot: the Objects pane owns the keyboard ─────────────
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")
        assert_eq(q(tf, "focus_region"), "objects", "boot pane is the object list")
        assert_eq(q(tf, "prop_cursor"), None, "no Details property cursor yet")

        # Focus the widget by clicking an object row (a plain click also selects
        # just that one object — the cardinality-1 case for the keyboard edits).
        tf.click(path=f"{INSPECTOR}#0")
        wait_q(tf, "selected", 0, desc="click selects + focuses (Player)")
        assert_eq(q(tf, "row_count"), 7, "Player exposes 7 properties")

        # ── (B) keyboard region toggle + editing ─────────────────────
        # Tab moves the keyboard into the Details pane; entering it seeds the
        # property cursor at row 0.
        tf.key(path=INSPECTOR, name="Tab")
        wait_q(tf, "focus_region", "details", desc="Tab -> Details pane")
        assert_eq(q(tf, "prop_cursor"), 0, "entering Details seeds the cursor at row 0")

        # Tab again cycles back to Objects; a third Tab returns to Details.
        tf.key(path=INSPECTOR, name="Tab")
        wait_q(tf, "focus_region", "objects", desc="Tab -> back to Objects pane")
        tf.key(path=INSPECTOR, name="Tab")
        wait_q(tf, "focus_region", "details", desc="Tab -> Details again")
        assert_eq(q(tf, "prop_cursor"), 0, "the cursor persists across the region cycle")

        # Row 0 is Visible (Bool true). Space toggles it (the switch-activate key).
        assert_eq(q(tf, "value.0"), True, "Visible starts On")
        tf.key(path=INSPECTOR, name=" ")
        wait_q(tf, "value.0", False, desc="Space toggles the Bool cell at the cursor")
        tf.key(path=INSPECTOR, name="Enter")
        wait_q(tf, "value.0", True, desc="Enter also toggles the Bool cell")

        # ArrowDown moves the cursor to row 1 (Layer, an Int) — the spinbutton.
        tf.key(path=INSPECTOR, name="ArrowDown")
        wait_q(tf, "prop_cursor", 1, desc="ArrowDown -> property cursor 1 (Layer)")
        assert_eq(q(tf, "value.1"), 1, "Layer starts at 1")
        # ArrowRight / '+' step up, ArrowLeft / '-' step down (relative).
        tf.key(path=INSPECTOR, name="ArrowRight")
        wait_q(tf, "value.1", 2, desc="ArrowRight steps the numeric +1")
        tf.key(path=INSPECTOR, name="+")
        wait_q(tf, "value.1", 3, desc="'+' steps the numeric +1")
        tf.key(path=INSPECTOR, name="-")
        wait_q(tf, "value.1", 2, desc="'-' steps the numeric -1")
        tf.key(path=INSPECTOR, name="ArrowLeft")
        wait_q(tf, "value.1", 1, desc="ArrowLeft steps the numeric -1")

        # Modify Layer, then Delete resets the modified row to its default.
        tf.key(path=INSPECTOR, name="ArrowRight")
        wait_q(tf, "value.1", 2, desc="Layer stepped to 2")
        assert_eq(q(tf, "modified.1"), True, "Layer now diverges from its default")
        tf.key(path=INSPECTOR, name="Delete")
        wait_q(tf, "value.1", 1, desc="Delete resets the modified row to default")
        assert_eq(q(tf, "modified.1"), False, "Layer is back at its default")

        # Home / End jump the property cursor to the first / last row.
        tf.key(path=INSPECTOR, name="End")
        wait_q(tf, "prop_cursor", 6, desc="End -> last property row")
        tf.key(path=INSPECTOR, name="Home")
        wait_q(tf, "prop_cursor", 0, desc="Home -> first property row")

        # ── (C) RPC focus verbs: place + clamp the cursor ────────────
        tf.invoke("/external/select_all", None)
        assert_eq(q(tf, "row_count"), 3, "select_all -> 3 common rows")
        assert_eq(tf.invoke("/external/set_focus_region", "objects"), "objects",
                  "set_focus_region returns the read-back region")
        assert_eq(q(tf, "focus_region"), "objects", "region set to Objects over RPC")
        # focus_property clamps a too-large index to the last row AND focuses Details.
        assert_eq(tf.invoke("/external/focus_property", 99), 2,
                  "an out-of-range property cursor clamps to the last row")
        assert_eq(q(tf, "prop_cursor"), 2, "the clamped cursor is observable")
        assert_eq(q(tf, "focus_region"), "details", "focus_property focuses the Details pane")
        # A shrinking panel re-clamps the reported cursor; empty -> none.
        tf.invoke("/external/clear", None)
        assert_eq(q(tf, "row_count"), 0, "cleared selection -> no rows")
        assert_eq(q(tf, "prop_cursor"), None, "an empty panel reports no cursor")

        # ── (D) rejects ──────────────────────────────────────────────
        bad_region = False
        try:
            tf.invoke("/external/focus_region", "bogus")
        except RpcError:
            bad_region = True
        assert bad_region, "an unknown region token is a typed Rejected error"
        bad_type = False
        try:
            tf.invoke("/external/focus_region", 1)
        except RpcError:
            bad_type = True
        assert bad_type, "a non-string region arg is a typed TypeMismatch error"


if __name__ == "__main__":
    sys.exit(run_demo("R1224 inspector keyboard + focus a11y", body))
