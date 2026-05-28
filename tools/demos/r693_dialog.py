#!/usr/bin/env python3
"""R693 §5.16 §5.39 §5.40 §5.50 — modal Dialog + focus-trap end-to-end.

Drives the new `hello-dialog` binding via JSON-RPC + verifies the
modal-focus-trap substrate (`pinion_runtime::FocusManager`
`push_modal_scope` / `pop_modal_scope`, driven through
`pinion_core::modal_scope_request`) and the
`pinion_widget_paint::dialog` chrome end-to-end.

The dialog is a destructive-confirm modal (Cancel / Delete) built from
three real `ButtonExternal`s: the trigger `open_dialog` (primary) plus
the `dialog_ok` / `dialog_cancel` actions (extra externals). Opening
installs a focus trap over the action buttons; the scrim blocks the
background; Escape / Cancel / Delete close and restore focus to the
trigger.

R693 atomic verification scope (>=30 assertions):

  (A) closed shape — trigger present, no scrim / panel, idle status.
  (B) open via trigger click — scrim + panel + title + message + both
      action buttons appear; the scrim fills the viewport.
  (C) auto-focus — focus moves into the dialog onto `dialog_cancel`
      (the safe default for a destructive prompt).
  (D) Tab trap — focus/next + focus/prev cycle ONLY the action members
      and wrap inside them; focus/get reports the trap enumeration.
  (E) trap confinement — focus/set to the trigger (behind the scrim) is
      rejected; focus/set to a member succeeds.
  (F) scrim blocks — clicking the backdrop neither dismisses the dialog
      nor records a result (WAI-ARIA modal default, no light-dismiss).
  (G) Cancel button — click closes the dialog, records "cancelled",
      and restores focus to the trigger.
  (H) Delete (OK) button — re-open + click confirms, records "deleted".
  (I) Escape — re-open + Escape dismisses as a cancel.
  (J) keyboard activate — re-open, Tab to Delete, Enter confirms.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (520, 360)
PAUSE = 0.12

TRIGGER = "open_dialog"
OK = "dialog_ok"
CANCEL = "dialog_cancel"
SCRIM = "dialog_scrim"
PANEL = "dialog_panel"


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


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _status(snap) -> str:
    """The status line text (everything except button labels). Returns
    the first status-like sentence found in the paint scene."""
    return " | ".join(_all_text(snap))


def _focused(tf) -> str | None:
    return tf.request("focus/get").result.get("focused")


def _tab_order(tf) -> list:
    return tf.request("focus/get").result.get("tab_order") or []


def _focus_next(tf) -> str | None:
    return tf.request("focus/next").result.get("focused")


def _focus_prev(tf) -> str | None:
    return tf.request("focus/prev").result.get("focused")


def body() -> None:
    with RpcSubprocess("hello-dialog", boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, TRIGGER), "trigger button present when closed"
        assert not _present(snap, SCRIM), "no scrim when closed"
        assert not _present(snap, PANEL), "no panel when closed"
        text = _status(snap)
        assert "Delete file" in text, f"trigger label renders; got {text!r}"
        assert "No action taken yet" in text, f"idle status; got {text!r}"

        # ── (B) open via trigger click ──────────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, SCRIM), "scrim appears on open"
        assert _present(snap, PANEL), "panel appears on open"
        assert _present(snap, OK), "Delete action button present"
        assert _present(snap, CANCEL), "Cancel action button present"
        ptext = _all_text(snap)
        assert "Delete file?" in ptext, f"dialog title renders; got {ptext!r}"
        assert any("cannot be undone" in t for t in ptext), "dialog message renders"
        assert "Cancel" in ptext and "Delete" in ptext, "action labels render"
        scrim = find_by_tag(snap, SCRIM)
        scrim_alpha = int(((scrim.get("style") or {}).get("fill") or {}).get("a") or 0)
        assert scrim_alpha > 0, f"scrim paints a dim backdrop; alpha={scrim_alpha}"

        # ── (C) auto-focus into the dialog ──────────────────────────
        assert_eq(_focused(tf), CANCEL, "open auto-focuses the cancel action")
        order = _tab_order(tf)
        assert_eq(order, [CANCEL, OK], "focus/get reports the trap enumeration")

        # ── (D) Tab trap: cycle + wrap within members ───────────────
        assert_eq(_focus_next(tf), OK, "focus/next: cancel -> ok")
        assert_eq(_focus_next(tf), CANCEL, "focus/next wraps ok -> cancel")
        assert_eq(_focus_prev(tf), OK, "focus/prev wraps cancel -> ok")
        assert_eq(_focus_prev(tf), CANCEL, "focus/prev: ok -> cancel")

        # ── (E) trap confinement ────────────────────────────────────
        raised = False
        try:
            tf.request("focus/set", {"tag": TRIGGER})
        except RpcError:
            raised = True
        assert raised, "focus/set to the trigger behind the scrim is rejected"
        assert_eq(_focused(tf), CANCEL, "focus unchanged after rejected set")
        moved = tf.request("focus/set", {"tag": OK}).result.get("focused")
        assert_eq(moved, OK, "focus/set to a trap member succeeds")

        # ── (F) scrim blocks (no light-dismiss) ─────────────────────
        tf.click(path=SCRIM)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, PANEL), "backdrop click does NOT dismiss the modal"
        assert "No action taken yet" in _status(snap), "no result from backdrop click"

        # ── (G) Cancel button closes + restores focus ───────────────
        tf.click(path=CANCEL)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Cancel closes the dialog"
        assert not _present(snap, SCRIM), "scrim gone after cancel"
        assert "Deletion cancelled" in _status(snap), "cancel result recorded"
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger on close")
        assert_eq(_tab_order(tf), [TRIGGER], "base tab order restored after close")

        # ── (H) Delete (OK) button confirms ─────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(_focused(tf), CANCEL, "re-open auto-focuses cancel again")
        tf.click(path=OK)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Delete closes the dialog"
        assert "File deleted" in _status(snap), "accept result recorded"
        assert_eq(_focused(tf), TRIGGER, "focus restored after confirm")

        # ── (I) Escape dismisses as cancel ──────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert _present(tf.snapshot(source="paint", viewport=VIEWPORT), PANEL), "re-opened"
        tf.key(path=PANEL, name="Escape")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Escape dismisses the dialog"
        assert "Deletion cancelled" in _status(snap), "Escape records a cancel"
        assert_eq(_focused(tf), TRIGGER, "focus restored after Escape")

        # ── (J) keyboard activate: Tab to Delete, Enter confirms ────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(_focus_next(tf), OK, "Tab moves focus to Delete")
        tf.key(path=PANEL, name="Enter")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Enter on focused Delete confirms + closes"
        assert "File deleted" in _status(snap), "keyboard confirm records accept"
        assert_eq(_focused(tf), TRIGGER, "focus restored after keyboard confirm")


if __name__ == "__main__":
    sys.exit(run_demo("R693 §5.16 §5.39 §5.40 §5.50 — modal Dialog + focus trap", body))
