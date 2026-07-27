#!/usr/bin/env python3
"""R1462 §5.16 §5.39 §5.50 — a modal row names where focus goes (PINION-PR78).

Drives `hello-modal-refocus` over JSON-RPC. A modal command palette's row
dismisses the palette AND names a focus target from ONE user action — a
`modal_scope_request::close()` and a `focus_request::request(T)` in a
single dispatch frame.

Closing a trap restores the invoker unconditionally (WAI-ARIA's default,
`FocusManager::pop_modal_scope`). Until R1462 the shell drained the two
mailboxes intent-first, policy-second, so that automatic restore landed
ON TOP of the row's explicit request and always won. Both branches are
wrong and both are driven here:

  * invoker ALIVE  -> focus snapped back to the palette trigger instead of
    the field the command was about.
  * invoker GONE (the command replaced the view) -> the restore resolved
    to nothing and committed `None`: no keyboard focus anywhere.

Neither branch is visible in the picture — the palette closes and the
right view paints either way. The whole defect is the value of
`focus/get`, which is why every section below asserts on it.

Verification scope (>=40 assertions):

  (A) editor shape — trigger + field, no scrim, palette reports closed,
      the base enumeration is the editor's two controls.
  (B) open the palette — scrim + panel + both rows, the trap auto-focuses
      the first row, and the enumeration is the palette's members.
  (C) trap confinement — a `focus/set` to a control behind the scrim is
      refused; Tab cycles and wraps inside the two rows.
  (D) BRANCH 1, invoker alive — the row closes the palette and names the
      field. Focus must be on the FIELD. Pre-R1462: the trigger.
  (E) BRANCH 2, invoker gone — the row closes the palette, swaps the view
      (taking the trigger with it) and names the results control. Focus
      must be on that control. Pre-R1462: `None`, and the assertion that
      catches it is `focused is not None` — the terminal state a user
      cannot type their way out of.
  (F) the swap left no stranded scope — the base enumeration is the
      results view's controls and they are all reachable.
  (G) the plain dismiss is unchanged — Escape names no target, so the
      automatic restore is the entire answer and still returns the
      invoker. This is the control for (D)/(E): it fails if the fix had
      demoted the policy instead of making it overridable.

## What this demo does NOT cover

The one-arc guarantee (`External::on_focus_change` fires for the net
transition only, never for the invoker the pop transiently restores) is
NOT observable over RPC — the wire reports the focused tag, not the
sequence of notifications that produced it. That half is pinned by
`pinion_shell::substrate::modal_tail_focus_tests::
a_close_and_request_report_one_focus_arc` and its TUI mirror. Recorded
here rather than approximated with a check that cannot fail.
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

TRIGGER = "open_palette"
FIELD = "editor_field"
RESULT = "result_row"
BACK = "back_to_editor"
ROW_FIELD = "row_focus_field"
ROW_RESULTS = "row_show_results"

SCRIM = "palette_scrim"
PANEL = "palette_panel"
PALETTE_STATE = "palette_state"


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


def _palette_open(tf) -> bool:
    """The R795 query-only modal introspect — 'is the palette up?' as
    data, no pixels (§2 #7)."""
    return tf.query(f"/{PALETTE_STATE}/external/open")


def _refused(tf, tag: str) -> bool:
    """True when `focus/set` to `tag` is rejected by the focus manager."""
    try:
        tf.request("focus/set", {"tag": tag})
    except RpcError:
        return True
    return False


def _open_palette(tf) -> None:
    """Click the trigger and wait for the trap to install."""
    tf.click(path=TRIGGER)
    wait_until(lambda: _focused(tf) == ROW_FIELD, desc="palette trap installs")


def body() -> None:
    with RpcSubprocess("hello-modal-refocus", boot_grace=1.5) as tf:
        # ── (A) editor shape ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, TRIGGER), "the palette trigger is painted"
        assert _present(snap, FIELD), "the editor field is painted"
        assert not _present(snap, SCRIM), "no scrim when the palette is closed"
        assert not _present(snap, RESULT), "the results view is not painted yet"
        text = _all_text(snap)
        assert any("Commands" in t for t in text), f"trigger label; got {text!r}"
        assert any("No command run yet" in t for t in text), "idle status"
        assert_eq(_palette_open(tf), False, "the palette reports closed")
        assert_eq(
            _tab_order(tf),
            [TRIGGER, FIELD],
            "the base enumeration is the editor's two controls",
        )

        # The invoker: whatever holds focus when the palette opens is what
        # `push_modal_scope` snapshots for restore. Assert it explicitly —
        # both branches below are claims ABOUT this tag, so a demo that
        # left it implicit could pass while describing a different state.
        assert_eq(
            tf.request("focus/set", {"tag": TRIGGER}).result.get("focused"),
            TRIGGER,
            "focus rests on the trigger, so IT is the invoker to restore",
        )

        # ── (B) open the palette ────────────────────────────────────
        tf.click(path=TRIGGER)
        snap = wait_snap(
            tf,
            lambda s: _present(s, SCRIM),
            viewport=VIEWPORT,
            desc="palette scrim appears",
        )
        assert _present(snap, PANEL), "palette panel appears"
        assert _present(snap, ROW_FIELD) and _present(snap, ROW_RESULTS), "both rows"
        ptext = _all_text(snap)
        assert "Focus the field" in ptext, f"row 1 label; got {ptext!r}"
        assert "Show results" in ptext, f"row 2 label; got {ptext!r}"
        assert_eq(_palette_open(tf), True, "the palette reports open")
        assert_eq(_focused(tf), ROW_FIELD, "the trap auto-focuses its first row")
        assert_eq(
            _tab_order(tf),
            [ROW_FIELD, ROW_RESULTS],
            "the palette owns the active enumeration",
        )

        # ── (C) trap confinement ────────────────────────────────────
        assert _refused(tf, TRIGGER), "the trigger behind the scrim is unreachable"
        assert _refused(tf, FIELD), "the field behind the scrim is unreachable"
        assert_eq(_focused(tf), ROW_FIELD, "focus unmoved by the refused sets")
        assert_eq(
            tf.request("focus/next").result.get("focused"),
            ROW_RESULTS,
            "Tab: row 1 -> row 2",
        )
        assert_eq(
            tf.request("focus/next").result.get("focused"),
            ROW_FIELD,
            "Tab wraps inside the palette",
        )

        # ── (D) BRANCH 1 — the invoker survives the command ─────────
        # ONE click writes close() and request(FIELD). The restore target
        # (the trigger) is still on screen, so the automatic policy has a
        # perfectly valid answer — it is simply not the answer the binding
        # gave. Pre-R1462 the policy won and focus landed on TRIGGER.
        tf.click(path=ROW_FIELD)
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="the row dismisses the palette",
        )
        assert not _present(snap, SCRIM), "the scrim goes with the panel"
        assert any("Focused the field." in t for t in _all_text(snap)), "outcome"
        assert_eq(_palette_open(tf), False, "the palette reports closed")
        assert _present(snap, TRIGGER), (
            "the invoker is STILL PAINTED — this branch is not about a "
            "vanished node, it is about policy outranking intent"
        )
        assert_eq(
            _focused(tf),
            FIELD,
            "the row named the field; landing on the trigger here is the "
            "automatic restore beating the binding's explicit request",
        )
        assert_eq(
            _tab_order(tf),
            [TRIGGER, FIELD],
            "the base enumeration is back, whole",
        )

        # ── (E) BRANCH 2 — the command takes the invoker with it ────
        # Re-open, then run the row that swaps the view. The trigger is
        # painted by the editor view only, so it leaves the scene in the
        # same frame; `pop_modal_scope` filters its saved tag out and
        # commits `None`. Pre-R1462 that `None` overwrote the request and
        # the window was left with NO keyboard focus.
        _open_palette(tf)
        assert_eq(_focused(tf), ROW_FIELD, "trap re-installed over the rows")
        tf.click(path=ROW_RESULTS)
        snap = wait_snap(
            tf,
            lambda s: _present(s, RESULT),
            viewport=VIEWPORT,
            desc="the results view replaces the editor",
        )
        assert not _present(snap, PANEL), "the palette is gone"
        assert not _present(snap, TRIGGER), (
            "the invoker left the scene with the editor view — this is what "
            "makes the automatic restore resolve to nothing"
        )
        assert not _present(snap, FIELD), "the editor's field went with it"
        assert _present(snap, BACK), "the results view's way back is painted"
        assert any("Showed results." in t for t in _all_text(snap)), "outcome"

        focused = _focused(tf)
        assert focused is not None, (
            "focus is NOWHERE — the frame that destroyed the restore target "
            "also said where to go instead, and the shell dropped it. A user "
            "cannot type their way out of this state"
        )
        assert_eq(
            focused,
            RESULT,
            "the row named the results control and it is enumerable in the "
            "post-command view",
        )

        # ── (F) no stranded scope after the swap ────────────────────
        assert_eq(
            _tab_order(tf),
            [RESULT, BACK],
            "the base enumeration is the results view's controls",
        )
        assert not _refused(tf, BACK), "every control in the new view is reachable"
        assert not _refused(tf, RESULT), "including the one focus landed on"
        assert_eq(_palette_open(tf), False, "no palette survived the swap")

        # ── (G) the plain dismiss still restores the invoker ────────
        # Back to the editor, open the palette, and Escape out of it. This
        # frame writes a modal edit and NO request, so the automatic policy
        # is the entire answer. R1462 made the policy overridable, not
        # weaker — if it had demoted the restore, this section fails while
        # (D) and (E) still pass.
        tf.click(path=BACK)
        wait_until(lambda: _focused(tf) == TRIGGER, desc="back in the editor")
        assert_eq(_tab_order(tf), [TRIGGER, FIELD], "editor enumeration back")

        _open_palette(tf)
        tf.key(path=PANEL, name="Escape")
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Escape dismisses the palette",
        )
        assert any("Dismissed." in t for t in _all_text(snap)), "dismiss recorded"
        assert_eq(
            _focused(tf),
            TRIGGER,
            "a dismiss that names no target restores the invoker — the "
            "WAI-ARIA default is intact",
        )
        assert_eq(_tab_order(tf), [TRIGGER, FIELD], "no scope stranded by Escape")
        assert not _refused(tf, FIELD), "background focusable after the Escape path"


if __name__ == "__main__":
    sys.exit(run_demo("R1462 §5.16 §5.39 §5.50 — modal refocus (PINION-PR78)", body))
