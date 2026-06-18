#!/usr/bin/env python3
"""R989 §5.16 §5.38 §5.40 — contextual selection-action toolbar.

A multi-select list (`sel_list`) over a shared holder plus an action toolbar
(`actions`) of three commands (Delete / Clear / Select all). The toolbar's
disabled state is REFLECTIVE: the view recomputes it each frame from the
selection and lowers it to `aria-disabled`. Delete / Clear grey out while
nothing is selected; Select all greys out when everything is. A disabled action
is a no-op however it is driven (the reducer gates on the same mask).

Verification scope (>=30 assertions; gates per [[zero-flake-policy]]: every
action->assert edge polls observed state, no fixed sleeps):

  (A) boot — 8 rows, nothing selected, count readout, Delete/Clear disabled.
  (B) click a row -> selected; Delete/Clear become operable (aria-disabled gone).
  (C) toggle a second row, then toggle the first back off.
  (D) Select all -> every row selected, Select all disabled.
  (E) Clear -> nothing selected, Delete/Clear disabled again.
  (F) Delete selected -> the collection shrinks; survivors keep stable ids.
  (G) a disabled action is a no-op even when clicked.
  (H) keyboard list: focus, rove, Space toggles the focused row.
  (I) keyboard toolbar: focus, Enter activates the focused (operable) action.
  (J) scene/access — toolbar aria-disabled + listbox aria-selected as data.
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
    wait_query,
    wait_until,
)

VIEWPORT = (460, 520)
KEY_AT = (5.0, 5.0)

DELETE, CLEAR, SELECT_ALL = 0, 1, 2


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _text_of(tf, tag: str) -> str:
    node = find_by_tag(_paint(tf), tag)
    return (node.get("content") or "") if node else ""


def _access(tf):
    """`scene/access` -> {tag: node} over the enriched a11y tree."""
    nodes = tf.request("scene/access").result["nodes"]
    return {n.get("tag"): n for n in nodes if n.get("tag")}


def _action_disabled(tf, action: int) -> bool:
    """True iff the toolbar action lowers `aria-disabled`. `scene/access` nests
    interaction flags under a `state` object, omitting unset ones, so an enabled
    control has no `state.disabled` key."""
    node = _access(tf).get(f"actions#{action}", {})
    return bool(node.get("state", {}).get("disabled", False))


def _wait_count(tf, n: int, desc: str) -> None:
    wait_query(tf, "/external/selected_count", n, desc=desc)


def _click_action(tf, action: int) -> None:
    tf.click(path=f"actions#{action}")
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess("hello-selection-toolbar", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        wait_until(lambda: find_by_tag(_paint(tf), "sel_list#1") is not None, desc="list paints")
        assert_eq(tf.query("/external/count"), 8, "boot: eight rows")                     # 1
        assert_eq(tf.query("/external/selected_count"), 0, "boot: nothing selected")      # 2
        assert_eq(tf.query("/external/ids_selected"), [], "boot: empty selection set")    # 3
        assert "Nothing selected" in _text_of(tf, "sel_count"), "boot count readout"      # 4
        assert _action_disabled(tf, DELETE), "boot: Delete disabled (nothing to delete)"  # 5
        assert _action_disabled(tf, CLEAR), "boot: Clear disabled"                         # 6
        assert not _action_disabled(tf, SELECT_ALL), "boot: Select all operable"          # 7

        acc = _access(tf)
        assert_eq(acc["actions"]["role"], "toolbar", "the action bar is a toolbar")        # 8
        assert_eq(acc["sel_list"]["role"], "listbox", "the list is a listbox")             # 9
        assert acc["sel_list"].get("multiselectable"), "listbox is multiselectable"        # 10
        assert_eq(acc["actions#0"]["name"], "Delete", "Delete control names itself")       # 11

        # ── (B) click a row -> selected; Delete/Clear become operable ─
        tf.click(path="sel_list#3")
        _wait_count(tf, 1, "clicking row 3 selects it")                                    # 12
        assert_eq(tf.query("/external/ids_selected"), [3], "id 3 is the selection")        # 13
        assert not _action_disabled(tf, DELETE), "Delete operable with a selection"        # 14
        assert not _action_disabled(tf, CLEAR), "Clear operable with a selection"          # 15
        assert "1 of 8 selected" in _text_of(tf, "sel_count"), "count readout updates"     # 16
        assert _access(tf)["sel_list#3"].get("selected") is True, "row 3 is aria-selected" # 17

        # ── (C) second row, then toggle the first back off ───────────
        tf.click(path="sel_list#5")
        _wait_count(tf, 2, "clicking row 5 adds it")                                       # 18
        assert_eq(tf.query("/external/ids_selected"), [3, 5], "two rows selected")         # 19
        tf.click(path="sel_list#3")
        _wait_count(tf, 1, "clicking row 3 again deselects it")                            # 20
        assert_eq(tf.query("/external/ids_selected"), [5], "only row 5 remains selected")  # 21

        # ── (D) Select all ───────────────────────────────────────────
        _click_action(tf, SELECT_ALL)
        _wait_count(tf, 8, "Select all selects every row")                                 # 22
        assert _action_disabled(tf, SELECT_ALL), "Select all disabled once all selected"   # 23
        assert not _action_disabled(tf, DELETE), "Delete operable when all selected"       # 24

        # ── (E) Clear ────────────────────────────────────────────────
        _click_action(tf, CLEAR)
        _wait_count(tf, 0, "Clear deselects every row")                                    # 25
        assert _action_disabled(tf, DELETE), "Delete disabled again after Clear"           # 26

        # ── (F) Delete selected -> the collection shrinks ────────────
        tf.click(path="sel_list#1")
        tf.click(path="sel_list#2")
        _wait_count(tf, 2, "select rows 1 and 2 to delete")                                # 27
        _click_action(tf, DELETE)
        wait_query(tf, "/external/count", 6, desc="Delete removes the two selected rows")  # 28
        assert_eq(tf.query("/external/selected_count"), 0, "selection cleared by delete")  # 29
        assert find_by_tag(_paint(tf), "sel_list#1") is None, "deleted row 1 is gone"      # 30
        assert find_by_tag(_paint(tf), "sel_list#3") is not None, "survivor keeps id 3"    # 31

        # ── (G) a disabled action is a no-op even when clicked ───────
        _click_action(tf, DELETE)  # nothing selected -> Delete is greyed
        # poll a real edge: the count must stay 6 (no row vanished).
        wait_until(lambda: tf.query("/external/count") == 6, desc="disabled Delete is a no-op")
        assert_eq(tf.query("/external/count"), 6, "disabled Delete deleted nothing")       # 32

        # ── (H) keyboard list: rove + Space toggles ──────────────────
        tf.request("focus/set", {"tag": "sel_list"})
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "sel_list",
                   desc="the list owns focus")                                             # 33
        tf.key(at=KEY_AT, name="ArrowDown")
        tf.key(at=KEY_AT, name="ArrowDown")
        wait_query(tf, "/external/focus", 2, desc="ArrowDown roves to row index 2")        # 34
        tf.key(at=KEY_AT, name="Space")
        _wait_count(tf, 1, "Space toggles the focused row")                                # 35
        assert_eq(tf.query("/external/selected.2"), True, "the focused row is selected")   # 36

        # ── (I) keyboard toolbar: focus + Enter activates Delete ─────
        tf.request("focus/set", {"tag": "actions"})
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "actions",
                   desc="the toolbar owns focus")                                          # 37
        assert_eq(tf.query("/actions/external/focus"), 0, "roving cursor starts on Delete") # 38
        tf.key(at=KEY_AT, name="Enter")  # Delete is operable (one row selected)
        wait_query(tf, "/external/count", 5, desc="Enter on Delete removes the selected row")  # 39
        assert_eq(tf.query("/external/selected_count"), 0, "keyboard delete clears selection")  # 40

        # ── (J) scene/access final cross-check ───────────────────────
        acc = _access(tf)
        assert _action_disabled(tf, DELETE), "Delete disabled again (empty selection)"     # 41
        sel_options = [t for t, n in acc.items()
                       if n.get("role") == "option" and n.get("selected") is True]
        assert_eq(sel_options, [], "no row is aria-selected after the keyboard delete")     # 42


if __name__ == "__main__":
    sys.exit(run_demo("R989 §5.16 §5.38 §5.40 — contextual selection-action toolbar", body))
