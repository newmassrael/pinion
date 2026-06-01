#!/usr/bin/env python3
"""R694 §5.16 §5.39 §5.50 — keyboard focus-ring substrate end-to-end.

Verifies that the shell-focus posture now reaches the paint layer so a
keyboard-focused control draws a visible focus ring — the cross-widget
"shell-focus-paint" debt the R690 Tabs / R692 Toolbar / R693 Dialog
rounds deferred. The substrate threads the External's focus posture
(mirrored by the shell via `External::on_focus_change`) through the
`focused` introspect slot into the binding's `read_state`, the same
channel hover / pressed already use; `view_button` (width-3 ring) and
`view_toolbar` (width-2 roving ring) paint a `Border` when focused.

The ring is a paint detail, but it is RPC-observable: `scene/snapshot`
serialises each container's `style.border` (null when absent), so an AI
client can confirm exactly which control the keyboard focus sits on
without OCR (§2 #7 scene-as-data).

R743 — the action→snapshot points poll via `wait_until` instead of a
fixed sleep: an RPC focus change applies on the next shell frame and
`scene/snapshot from=paint` reads the last *rendered* frame (R705), so a
fixed sleep raced the render under full-sweep load (the intermittent
"ring follows focus" flake). Polling on the observed ring makes the demo
deterministic whatever the machine load.

Atomic verification scope (>=30 assertions):

  hello-dialog (Button-based action buttons — the named 3rd consumer):
    (A) open + auto-focus Cancel paints a ring on Cancel only.
    (B) the ring is the M3 width-3 indicator (FOCUS_RING_WIDTH).
    (C) focus/next moves the ring Cancel -> Delete (and back).
    (D) the trigger behind the scrim never rings.
    (E) closing clears every ring.

  hello-toolbar (roving-cursor strip — group_focused gating):
    (F) the roving control rings only while the strip owns shell focus.
    (G) the ring is the width-2 toolbar indicator on the roving control.
    (H) Arrow moves the roving cursor and the ring follows it.
    (I) non-roving controls never ring.
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
    wait_until,
)

DIALOG_VIEWPORT = (520, 360)
TOOLBAR_VIEWPORT = (640, 200)

TRIGGER = "open_dialog"
OK = "dialog_ok"
CANCEL = "dialog_cancel"
PANEL = "dialog_panel"

TOOLBAR = "toolbar"
BUTTON_RING_WIDTH = 3
TOOLBAR_RING_WIDTH = 2


def _border(snap, tag):
    """The `style.border` of the container tagged `tag` (None when the
    node is absent or carries no border)."""
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    return (node.get("style") or {}).get("border")


def _has_ring(snap, tag) -> bool:
    return _border(snap, tag) is not None


def _ring_width(snap, tag):
    border = _border(snap, tag)
    return None if border is None else border.get("width")


def _focused(tf):
    return tf.request("focus/get").result.get("focused")


def _composite(index: int) -> str:
    return f"{TOOLBAR}#{index}"


def _dialog() -> None:
    with RpcSubprocess("hello-dialog", boot_grace=1.5) as tf:
        def snap_when(pred, desc):
            """Re-snapshot the paint scene until `pred(snap)` holds
            (robust under load), returning the settled snapshot."""
            return wait_until(
                lambda: (lambda s: s if pred(s) else None)(
                    tf.snapshot(source="paint", viewport=DIALOG_VIEWPORT)
                ),
                desc=desc,
            )

        # ── (A) open + auto-focus Cancel rings Cancel only ──────────
        tf.click(path=TRIGGER)
        wait_until(lambda: _focused(tf) == CANCEL, desc="open auto-focuses Cancel")
        assert_eq(_focused(tf), CANCEL, "open auto-focuses Cancel")
        snap = snap_when(lambda s: _has_ring(s, CANCEL), "focused Cancel paints a ring")
        assert _has_ring(snap, CANCEL), "focused Cancel paints a focus ring"
        assert not _has_ring(snap, OK), "unfocused Delete has no ring"

        # ── (B) ring is the M3 width-3 indicator ────────────────────
        assert_eq(_ring_width(snap, CANCEL), BUTTON_RING_WIDTH, "M3 button ring width")
        cancel_border = _border(snap, CANCEL)
        assert cancel_border.get("color") is not None, "ring carries a colour"

        # ── (C) focus/next moves the ring Cancel -> Delete ──────────
        assert_eq(tf.request("focus/next").result.get("focused"), OK, "Tab -> Delete")
        snap = snap_when(
            lambda s: _has_ring(s, OK) and not _has_ring(s, CANCEL),
            "ring follows focus to Delete",
        )
        assert _has_ring(snap, OK), "ring follows focus to Delete"
        assert not _has_ring(snap, CANCEL), "ring left Cancel"
        assert_eq(_ring_width(snap, OK), BUTTON_RING_WIDTH, "Delete ring width")

        # back to Cancel (wrap)
        assert_eq(tf.request("focus/next").result.get("focused"), CANCEL, "wrap -> Cancel")
        snap = snap_when(
            lambda s: _has_ring(s, CANCEL) and not _has_ring(s, OK),
            "ring back on Cancel after wrap",
        )
        assert _has_ring(snap, CANCEL), "ring back on Cancel after wrap"
        assert not _has_ring(snap, OK), "Delete ring cleared on wrap"

        # ── (D) the trigger behind the scrim never rings ────────────
        assert not _has_ring(snap, TRIGGER), "trigger behind scrim shows no ring"
        raised = False
        try:
            tf.request("focus/set", {"tag": TRIGGER})
        except RpcError:
            raised = True
        assert raised, "focus cannot escape the modal trap to the trigger"
        snap = tf.snapshot(source="paint", viewport=DIALOG_VIEWPORT)
        assert not _has_ring(snap, TRIGGER), "trigger still ringless after rejected set"

        # ── (E) closing clears the action rings; focus + ring return
        #        to the trigger (a standalone button rings when focused
        #        too — the substrate is not dialog-specific) ───────────
        tf.key(path=PANEL, name="Escape")
        wait_until(lambda: _focused(tf) == TRIGGER, desc="focus restored to the trigger")
        snap = snap_when(
            lambda s: find_by_tag(s, CANCEL) is None and _has_ring(s, TRIGGER),
            "close clears the dialog and re-rings the trigger",
        )
        assert find_by_tag(snap, CANCEL) is None, "Cancel button gone after close"
        assert find_by_tag(snap, OK) is None, "Delete button gone after close"
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger")
        assert _has_ring(snap, TRIGGER), "the re-focused trigger paints a ring"
        assert_eq(_ring_width(snap, TRIGGER), BUTTON_RING_WIDTH, "trigger ring width")


def _toolbar() -> None:
    with RpcSubprocess("hello-toolbar", boot_grace=1.5) as tf:
        def snap_when(pred, desc):
            return wait_until(
                lambda: (lambda s: s if pred(s) else None)(
                    tf.snapshot(source="paint", viewport=TOOLBAR_VIEWPORT)
                ),
                desc=desc,
            )

        # ── (F) the roving control rings only while the strip is the
        #        focused widget ───────────────────────────────────────
        # before focus, the strip does not own shell focus: the `focused`
        # introspect slot is false (the substrate the ring reads through).
        assert_eq(tf.query("/external/focused"), False, "strip boots without group focus")
        tf.request("focus/set", {"tag": TOOLBAR})
        wait_until(lambda: _focused(tf) == TOOLBAR, desc="toolbar takes shell focus")
        assert_eq(_focused(tf), TOOLBAR, "toolbar takes shell focus")
        assert_eq(tf.query("/external/focused"), True, "`focused` slot reports group focus")
        snap = snap_when(
            lambda s: _has_ring(s, _composite(0)),
            "roving control rings while group-focused",
        )
        assert _has_ring(snap, _composite(0)), "roving control rings while group-focused"

        # ── (G) ring is the width-2 toolbar indicator ───────────────
        assert_eq(_ring_width(snap, _composite(0)), TOOLBAR_RING_WIDTH, "toolbar ring width")
        assert _border(snap, _composite(0)).get("color") is not None, "ring carries a colour"

        # ── (I) non-roving controls never ring ──────────────────────
        assert not _has_ring(snap, _composite(1)), "non-roving control #1 ringless"
        assert not _has_ring(snap, _composite(2)), "non-roving control #2 ringless"
        assert not _has_ring(snap, _composite(3)), "non-roving control #3 ringless"
        assert not _has_ring(snap, _composite(4)), "non-roving control #4 ringless"

        # ── (H) Arrow moves the roving cursor; the ring follows ─────
        tf.key(path=TOOLBAR, name="ArrowRight")
        snap = snap_when(
            lambda s: _has_ring(s, _composite(1)) and not _has_ring(s, _composite(0)),
            "ring follows the roving cursor right",
        )
        assert _has_ring(snap, _composite(1)), "ring follows the roving cursor right"
        assert not _has_ring(snap, _composite(0)), "ring left control #0"
        assert_eq(_ring_width(snap, _composite(1)), TOOLBAR_RING_WIDTH, "moved ring width")

        tf.key(path=TOOLBAR, name="ArrowRight")
        snap = snap_when(
            lambda s: _has_ring(s, _composite(2)) and not _has_ring(s, _composite(1)),
            "ring follows the roving cursor again",
        )
        assert _has_ring(snap, _composite(2)), "ring follows the roving cursor again"
        assert not _has_ring(snap, _composite(1)), "ring left control #1"

        tf.key(path=TOOLBAR, name="ArrowLeft")
        snap = snap_when(
            lambda s: _has_ring(s, _composite(1)) and not _has_ring(s, _composite(2)),
            "ArrowLeft moves the ring back",
        )
        assert _has_ring(snap, _composite(1)), "ArrowLeft moves the ring back"
        assert not _has_ring(snap, _composite(2)), "ring left control #2"


def body() -> None:
    _dialog()
    _toolbar()


if __name__ == "__main__":
    sys.exit(run_demo("R694 §5.16 §5.39 §5.50 — keyboard focus ring", body))
