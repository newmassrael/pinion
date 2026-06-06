#!/usr/bin/env python3
"""R805 §5.40 §5.38 — stateful menu items (checkbox / separator / disabled).

Drives the `hello-menu` binding's R805 item taxonomy over the
command-class `MenuBarExternal`, end-to-end via JSON-RPC:

  View = [checkbox "Show Grid" (on), checkbox "Show Rulers" (off),
          separator, command "Zoom In", "Zoom Out", "Reset Zoom"]
  Edit = [command "Undo", DISABLED command "Redo", separator,
          command "Cut", "Copy", "Paste"]

Verification axis (>=30 assertions):

  (A) introspection — `item_kind.<m>.<i>` reports command / checkbox /
      separator; `checked.<m>.<i>` reports the toggle state; `enabled`
      reports the disabled Redo.
  (B) paint — the checked checkbox paints the check glyph; an unchecked
      one does not.
  (C) toggle — activating a checkbox flips its persistent `checked`,
      emits the `menu.command` intent, and closes; the new state
      survives close / reopen.
  (D) keyboard nav skips the separator (and would skip a disabled row):
      ArrowDown steps 0 -> 1 -> 3 (past the separator at 2).
  (E) inert rows — activating the separator or the disabled Redo fires
      no command and leaves the menu open.
  (F) intervene — `checked.<m>.<i>` sets a toggle programmatically with
      no command intent (RPC restore).
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
)

VIEWPORT = (520, 320)
CHECK_GLYPH = "✓"
PAUSE = 0.12


def _all_text(node) -> list[str]:
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                content = n.get("content")
                if isinstance(content, str):
                    out.append(content)
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return out


def _dropdown_text(tf) -> list[str]:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return _all_text(find_by_tag(snap, "menu_dropdown"))


def _assert_no_command(tf, label: str) -> None:
    """An inert row (separator / disabled) must NOT log a command. The
    command intent drains to stderr as `shell: intent menu.command ...`
    (the §5.20 single-consumer path, see r691_menu.py)."""
    tail = tf.stderr_tail(6)
    assert not any("shell: intent menu.command" in ln for ln in tail), (
        f"{label}: an inert row fired a command; stderr={tail!r}"
    )


def _open(tf, m: int) -> None:
    tf.click(path=f"menu#t{m}")
    time.sleep(PAUSE)
    assert_eq(tf.query("/external/open"), m, f"menu {m} open")


def body() -> None:
    with RpcSubprocess("hello-menu", boot_grace=1.5) as tf:
        # ── (A) introspection: the View taxonomy ────────────────────
        assert_eq(tf.query("/external/item_kind.2.0"), "checkbox", "View 0 is a checkbox")
        assert_eq(tf.query("/external/item_kind.2.1"), "checkbox", "View 1 is a checkbox")
        assert_eq(tf.query("/external/item_kind.2.2"), "separator", "View 2 is a separator")
        assert_eq(tf.query("/external/item_kind.2.3"), "command", "View 3 is a command")
        assert_eq(tf.query("/external/checked.2.0"), True, "Show Grid boots checked")
        assert_eq(tf.query("/external/checked.2.1"), False, "Show Rulers boots unchecked")
        assert_eq(tf.query("/external/enabled.2.0"), True, "Show Grid is enabled")
        # Edit's disabled Redo + separator.
        assert_eq(tf.query("/external/item_kind.1.1"), "command", "Edit 1 (Redo) is a command")
        assert_eq(tf.query("/external/enabled.1.1"), False, "Redo is disabled")
        assert_eq(tf.query("/external/item_kind.1.2"), "separator", "Edit 2 is a separator")
        assert_eq(tf.query("/external/enabled.1.0"), True, "Undo is enabled")

        # ── (B) paint: checked checkbox shows the glyph ─────────────
        _open(tf, 2)
        text = _dropdown_text(tf)
        assert CHECK_GLYPH in text, f"checked Show Grid paints the check glyph; got {text!r}"
        assert text.count(CHECK_GLYPH) == 1, "only the one checked item shows a glyph"
        assert "Show Grid" in text and "Show Rulers" in text, "both checkbox labels render"
        assert "Zoom In" in text, "the command group renders past the separator"

        # ── (C) toggle Show Grid off: flips, emits command, closes ──
        tf.click(path="menu#i0")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), None, "activating a checkbox closes the menu")
        tail = tf.stderr_tail(80)
        assert any('menu.command payload=Text("2.0")' in ln for ln in tail), (
            f"checkbox activation emits the command intent; stderr={tail[-5:]!r}"
        )
        assert_eq(tf.query("/external/checked.2.0"), False, "Show Grid toggled off")
        # Reopen — the new state persisted, and no glyph paints now.
        _open(tf, 2)
        assert_eq(tf.query("/external/checked.2.0"), False, "toggle persists across reopen")
        assert CHECK_GLYPH not in _dropdown_text(tf), "no glyph after toggling off"
        # Toggle Show Rulers on.
        tf.click(path="menu#i1")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/checked.2.1"), True, "Show Rulers toggled on")

        # ── (D) keyboard nav skips the separator ────────────────────
        tf.request("focus/set", {"tag": "menu"})
        time.sleep(0.1)
        _open(tf, 2)
        assert_eq(tf.query("/external/active"), None, "mouse-open pre-highlights nothing")
        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 0, "ArrowDown -> first item (0)")
        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 1, "ArrowDown -> 1 (Show Rulers)")
        tf.key(path="menu", name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 3, "ArrowDown skips the separator (2) -> 3")
        tf.key(path="menu", name="Home")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), 0, "Home -> first navigable")
        tf.key(path="menu", name="Escape")
        time.sleep(PAUSE)

        # ── (E) inert rows: separator + disabled fire no command ────
        _open(tf, 2)
        tf.invoke("/external/send", "i2:PointerEnter")  # hover the separator
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), None, "hover does not highlight a separator")
        tf.click(path="menu#i2")  # activate the separator
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 2, "activating a separator leaves the menu open")
        _assert_no_command(tf, "separator activate")
        tf.key(path="menu", name="Escape")
        time.sleep(PAUSE)
        # Disabled Redo in Edit.
        _open(tf, 1)
        tf.invoke("/external/send", "i1:PointerEnter")  # hover disabled Redo
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/active"), None, "hover does not highlight a disabled row")
        tf.click(path="menu#i1")  # activate disabled Redo
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/open"), 1, "activating a disabled row leaves the menu open")
        _assert_no_command(tf, "disabled activate")
        tf.key(path="menu", name="Escape")
        time.sleep(PAUSE)

        # ── (F) intervene sets a toggle programmatically, no command ─
        tf.intervene("/external/checked.2.0", True)
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/checked.2.0"), True, "intervene set checked")
        _assert_no_command(tf, "intervene checked")

        # ── (G) R805.1 — intervene checked on a non-checkbox is rejected ─
        # View item 3 (Zoom In) is a command, item 2 a separator: neither
        # has a writable checked slot, so the RPC surface cannot reach a
        # nonsensical "checked command" state (sum-type model + ReadOnly).
        rejected = False
        try:
            tf.intervene("/external/checked.2.3", True)  # Zoom In = command
        except Exception:  # noqa: BLE001 - RpcError surfaces the ReadOnly reject
            rejected = True
        assert rejected, "intervene checked on a command must be rejected"
        assert_eq(tf.query("/external/checked.2.3"), False, "command stays unchecked")
        rejected_sep = False
        try:
            tf.intervene("/external/checked.2.2", True)  # separator
        except Exception:  # noqa: BLE001
            rejected_sep = True
        assert rejected_sep, "intervene checked on a separator must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R805 §5.40 — stateful menu items", body))
