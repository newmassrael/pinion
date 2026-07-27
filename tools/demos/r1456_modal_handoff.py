#!/usr/bin/env python3
"""R1456 §5.16 §5.39 §5.50 — modal HANDOFF end-to-end (PINION-PR77).

Drives `hello-modal-handoff` over JSON-RPC. A modal command menu's
destructive row closes the menu AND opens a confirm dialog from ONE user
action — two `modal_scope_request` edits in a single dispatch frame, the
shape a single last-write-wins slot could not express.

The failure this pins is **delayed**: while the confirm is up, a leaked
menu scope is invisible (the confirm's members are the active
enumeration either way). It surfaces only once the confirm is answered —
the ghost scope underneath becomes the active enumeration, so the
background is permanently unfocusable and every `focus/set` is refused.
Section (F) is therefore the counterfactual anchor: with the fix
reverted, (F) fails while everything before it still passes.

Verification scope (>=30 assertions):

  (A) closed shape — trigger only, no scrim / panel, both modals report
      `open == false` over the query-only introspect.
  (B) open the menu — scrim + panel + both rows; the trap auto-focuses
      the first row and `focus/get` reports the menu's enumeration.
  (C) trap confinement — focus/set to the trigger behind the scrim is
      refused; Tab cycles and wraps inside the two rows.
  (D) THE HANDOFF — one click on the destructive row: the menu is gone
      from paint, the confirm is up, `menu_state.open == false` while
      `confirm_state.open == true`, and the active enumeration is the
      confirm's members.
  (E) the handoff did not stack — the confirm is trapped, and answering
      it pops the ONLY scope.
  (F) no ghost scope — after the confirm closes, focus is back on the
      trigger, `focus/get` reports the BASE enumeration, and `focus/set`
      to the trigger succeeds. This is the reported symptom.
  (G) the safe row is unchanged — a one-request frame still closes
      cleanly and leaves the base enumeration.
  (H) Escape on the handed-off confirm dismisses it without stranding a
      scope.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
    wait_until,
)

VIEWPORT = (560, 380)

TRIGGER = "open_menu"
RENAME = "menu_rename"
DELETE = "menu_delete"
CANCEL = "confirm_cancel"
OK = "confirm_ok"

MENU_SCRIM = "menu_scrim"
MENU_PANEL = "menu_panel"
CONFIRM_SCRIM = "confirm_scrim"
CONFIRM_PANEL = "confirm_panel"

MENU_STATE = "menu_state"
CONFIRM_STATE = "confirm_state"


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


def _focused(tf) -> str | None:
    return tf.request("focus/get").result.get("focused")


def _tab_order(tf) -> list:
    return tf.request("focus/get").result.get("tab_order") or []


def _modal_open(tf, tag: str) -> bool:
    """The R795 query-only modal introspect — 'is this surface up?' as
    data, no pixels (§2 #7)."""
    return tf.query(f"/{tag}/external/open")


def _refused(tf, tag: str) -> bool:
    """True when `focus/set` to `tag` is rejected by the focus manager."""
    try:
        tf.request("focus/set", {"tag": tag})
    except RpcError:
        return True
    return False


def body() -> None:
    with RpcSubprocess("hello-modal-handoff", boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, TRIGGER), "trigger present when nothing is open"
        assert not _present(snap, MENU_SCRIM), "no menu scrim when closed"
        assert not _present(snap, CONFIRM_SCRIM), "no confirm scrim when closed"
        text = _all_text(snap)
        assert any("Commands" in t for t in text), f"trigger label; got {text!r}"
        assert any("No command run yet" in t for t in text), "idle status"
        assert_eq(_modal_open(tf, MENU_STATE), False, "menu reports closed")
        assert_eq(_modal_open(tf, CONFIRM_STATE), False, "confirm reports closed")
        assert_eq(_tab_order(tf), [TRIGGER], "base enumeration is the trigger")

        # ── (B) open the menu ───────────────────────────────────────
        tf.click(path=TRIGGER)
        snap = wait_snap(
            tf,
            lambda s: _present(s, MENU_SCRIM),
            viewport=VIEWPORT,
            desc="menu scrim appears",
        )
        assert _present(snap, MENU_PANEL), "menu panel appears"
        assert _present(snap, RENAME), "safe row present"
        assert _present(snap, DELETE), "destructive row present"
        mtext = _all_text(snap)
        assert "Rename" in mtext and "Delete" in mtext, f"row labels; got {mtext!r}"
        assert_eq(_modal_open(tf, MENU_STATE), True, "menu reports open")
        assert_eq(_focused(tf), RENAME, "the menu auto-focuses its first row")
        assert_eq(_tab_order(tf), [RENAME, DELETE], "menu owns the enumeration")

        # ── (C) trap confinement ────────────────────────────────────
        assert _refused(tf, TRIGGER), "the trigger behind the scrim is unreachable"
        assert_eq(_focused(tf), RENAME, "focus unmoved by the refused set")
        assert_eq(
            tf.request("focus/next").result.get("focused"),
            DELETE,
            "Tab: rename -> delete",
        )
        assert_eq(
            tf.request("focus/next").result.get("focused"),
            RENAME,
            "Tab wraps inside the menu",
        )

        # ── (D) THE HANDOFF ─────────────────────────────────────────
        # ONE click. The reducer writes close() then open(); both edits
        # must reach the focus stack, in that order.
        tf.click(path=DELETE)
        snap = wait_snap(
            tf,
            lambda s: _present(s, CONFIRM_PANEL),
            viewport=VIEWPORT,
            desc="the confirm dialog takes the menu's place",
        )
        assert not _present(snap, MENU_PANEL), "the menu is gone from paint"
        assert not _present(snap, MENU_SCRIM), "the menu's scrim is gone too"
        assert _present(snap, CANCEL) and _present(snap, OK), "confirm actions present"
        ctext = _all_text(snap)
        assert "Delete file?" in ctext, f"confirm title; got {ctext!r}"
        assert any("cannot be undone" in t for t in ctext), "confirm message"
        assert_eq(_modal_open(tf, MENU_STATE), False, "menu reports closed")
        assert_eq(_modal_open(tf, CONFIRM_STATE), True, "confirm reports open")
        assert_eq(_focused(tf), CANCEL, "the confirm auto-focuses Cancel")
        assert_eq(_tab_order(tf), [CANCEL, OK], "confirm owns the enumeration")

        # ── (E) the handoff replaced, it did not stack ──────────────
        assert _refused(tf, RENAME), "the dismissed menu's row is NOT focusable"
        assert _refused(tf, TRIGGER), "the confirm still traps the background"
        assert_eq(
            tf.request("focus/set", {"tag": OK}).result.get("focused"),
            OK,
            "focus/set to a confirm member succeeds",
        )

        # ── (F) no ghost scope — the reported PR-77 symptom ─────────
        # Answering the confirm pops the ONLY scope. With the pre-R1456
        # last-write-wins mailbox the menu's scope was still underneath,
        # and every assertion below failed: the enumeration stayed
        # [menu_rename, menu_delete] and focus/set was refused forever.
        tf.click(path=OK)
        snap = wait_snap(
            tf,
            lambda s: not _present(s, CONFIRM_PANEL),
            viewport=VIEWPORT,
            desc="the confirm closes",
        )
        assert not _present(snap, CONFIRM_SCRIM), "no scrim survives the cycle"
        assert any("Deleted." in t for t in _all_text(snap)), "outcome recorded"
        assert_eq(_modal_open(tf, CONFIRM_STATE), False, "confirm reports closed")
        assert_eq(
            _tab_order(tf),
            [TRIGGER],
            "the BASE enumeration is active again — a menu member here is "
            "the ghost scope PINION-PR77 reported",
        )
        assert_eq(_focused(tf), TRIGGER, "focus restored to the invoker")
        assert not _refused(tf, TRIGGER), (
            "the background is focusable again — this returning refused is "
            "the unrecoverable symptom: the client's keyboard focus model "
            "is dead until restart"
        )

        # ── (G) the safe row is a one-request frame, unchanged ──────
        tf.click(path=TRIGGER)
        wait_until(lambda: _focused(tf) == RENAME, desc="menu re-opens")
        assert_eq(_tab_order(tf), [RENAME, DELETE], "menu trap re-installed")
        tf.click(path=RENAME)
        snap = wait_snap(
            tf,
            lambda s: not _present(s, MENU_PANEL),
            viewport=VIEWPORT,
            desc="the safe row closes the menu",
        )
        assert not _present(snap, CONFIRM_PANEL), "the safe row opens no dialog"
        assert any("Renamed." in t for t in _all_text(snap)), "safe outcome recorded"
        assert_eq(_tab_order(tf), [TRIGGER], "base enumeration back after one close")
        assert_eq(_focused(tf), TRIGGER, "focus restored after the safe row")

        # ── (H) Escape out of a handed-off confirm ──────────────────
        tf.click(path=TRIGGER)
        wait_until(lambda: _focused(tf) == RENAME, desc="menu re-opens again")
        tf.click(path=DELETE)
        wait_until(lambda: _focused(tf) == CANCEL, desc="handoff again")
        tf.key(path=CONFIRM_PANEL, name="Escape")
        snap = wait_snap(
            tf,
            lambda s: not _present(s, CONFIRM_PANEL),
            viewport=VIEWPORT,
            desc="Escape dismisses the handed-off confirm",
        )
        assert any("cancelled" in t for t in _all_text(snap)), "Escape records a cancel"
        assert_eq(_tab_order(tf), [TRIGGER], "no scope stranded by Escape")
        assert_eq(_focused(tf), TRIGGER, "focus restored after Escape")
        assert not _refused(tf, TRIGGER), "background focusable after the Escape path"


if __name__ == "__main__":
    sys.exit(run_demo("R1456 §5.16 §5.39 §5.50 — modal handoff (PINION-PR77)", body))
