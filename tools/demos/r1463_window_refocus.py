#!/usr/bin/env python3
"""R1463 §5.39 §5.16 — a focus request reaches a node that appeared in a
SECONDARY window.

Drives `hello-window-refocus` over JSON-RPC. Two OS windows share one
binding; the main window carries both triggers, so both branches travel
the identical input path and differ only in WHICH window paints the node
the reducer names:

    editing().set(...);                    # the editor becomes paintable
    pinion_core::focus_request::request(T) # ...and focus belongs in it

Scene-derived focus (R1020) enumerates focusable nodes from the painted
scene, so a node this dispatch just made paintable is not enumerated yet
— the paint that would enumerate it has not run. The shell handles that
by re-deriving the enumeration from a fresh view run and retrying the
request once. Until R1463 that retry re-derived the PRIMARY window only,
and a window that has painted answers from its harvested cache: the
identical reducer landed in the main window and was silently dropped in
the notes window, its one-shot request consumed by the miss.

Neither branch is visible in the picture — the editor paints either way,
in whichever window owns it. The whole defect is the value of
`focus/get`, which is why every section asserts on it.

Verification scope (>=36 assertions):

  (A) boot shape — two windows declared; main paints both triggers,
      notes paints its pane, neither editor is open.
  (B) the PREMISE, asserted rather than assumed — the notes window has
      really PAINTED (`scene/frame_timings` answers for it), which is
      exactly what makes its focus enumeration a cache; and the Tab order
      is the union across both windows.
  (C) CONTROL branch — the trigger whose editor appears in the MAIN
      window. This worked before R1463; it is here so the round's branch
      is measured against its own twin rather than against a claim.
  (D) the control's close path.
  (E) THE ROUND — the trigger whose editor appears in the NOTES window.
      Focus must be on that editor. Pre-R1463: the trigger that was just
      clicked, with the named node painted, focusable, and unreachable.
  (F) enumeration integrity — the editor joins the union exactly once,
      both windows' tags stay reachable, and the documented order
      (primary first, then the remaining windows) holds.
  (G) the close path from the secondary window's editor.

## What this demo does NOT cover

That the re-derive adds only a bounded amount of work (one view run per
PAINTED window, on a miss, never per frame). This demo reports the focused
tag, not how many view runs produced it.

R1464 closed that: `scene/frame_timings` now carries
`focus.derivations_total` / `focus.retries_total`, and
`tools/demos/r1464_focus_work.py` drives THIS example to assert the bound
as arithmetic on the same two windows. Read the pair together — this file
is the outcome, that one is the price.

Run from the workspace root:

    python3 tools/demos/r1463_window_refocus.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-window-refocus"

MAIN = "main"
NOTES = "notes"
MAIN_VIEWPORT = (420, 260)
NOTES_VIEWPORT = (360, 220)

EDIT_TITLE = "edit_title"
EDIT_NOTE = "edit_note"
TITLE_EDITOR = "title_editor"
NOTE_EDITOR = "note_editor"
NOTES_PANE = "notes_pane"
STATUS = "refocus_status"

BASE_ORDER = [EDIT_TITLE, EDIT_NOTE, NOTES_PANE]


def _snap(tf: RpcSubprocess, window: str) -> Any:
    viewport = MAIN_VIEWPORT if window == MAIN else NOTES_VIEWPORT
    return tf.snapshot(source="paint", viewport=viewport, window=window)


def _present(snap: Any, tag: str) -> bool:
    return find_by_tag(snap, tag) is not None


def _status(snap: Any) -> Optional[str]:
    node = find_by_tag(snap, STATUS)
    assert node is not None, "each window paints its tagged status line"
    return node.get("content")


def _focused(tf: RpcSubprocess) -> Optional[str]:
    return tf.request("focus/get").result.get("focused")


def _tab_order(tf: RpcSubprocess) -> list:
    return tf.request("focus/get").result.get("tab_order") or []


def _painted(tf: RpcSubprocess, window: str) -> bool:
    """True once `window` has produced a real frame.

    `scene/frame_timings` raises `FrameTimingsUnavailable` until the
    window paints, which makes it the honest witness for this demo's
    premise: a window that has painted answers the focus enumeration
    from its harvested cache, and that cache is what went stale.
    A `{window}`-scoped snapshot cannot say this — it RE-RUNS the view
    (and deliberately does not enumerate), so it answers even for a
    window that has never been on screen.
    """
    try:
        return int(tf.frame_timings(window=window)["frame_count"]) >= 1
    except RpcError:
        return False


def _refused(tf: RpcSubprocess, tag: str) -> bool:
    try:
        tf.request("focus/set", {"tag": tag})
    except RpcError:
        return True
    return False


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        windows = tf.request("scene/windows", {})
        declared = {w["id"] for w in windows.result["windows"]}
        assert MAIN in declared, "the main window is declared"
        assert NOTES in declared, "the notes window is declared"

        main = _snap(tf, MAIN)
        notes = _snap(tf, NOTES)
        assert _present(main, EDIT_TITLE), "main paints the title trigger"
        assert _present(main, EDIT_NOTE), "main paints the note trigger"
        assert not _present(main, TITLE_EDITOR), "no editor open at boot"
        assert not _present(main, NOTE_EDITOR), "the note editor is never in main"
        assert _present(notes, NOTES_PANE), "the notes window paints its pane"
        assert not _present(notes, NOTE_EDITOR), "no editor open at boot"
        assert not _present(notes, EDIT_TITLE), "the triggers live in main only"
        assert_eq(_status(main), "main: No editor open.", "main status at boot")
        assert_eq(_status(notes), "notes: No editor open.", "notes status at boot")

        # ── (B) the premise, measured ───────────────────────────────
        assert wait_until(
            lambda: _painted(tf, NOTES),
            desc="the notes window produces a real frame",
        ), "the secondary window has painted"
        assert _painted(tf, MAIN), "the primary window has painted"
        assert_eq(
            _tab_order(tf),
            BASE_ORDER,
            "the Tab order is the UNION across both windows: the primary's "
            "tags first, then the remaining windows by sorted id",
        )

        # ── (C) CONTROL — the editor appears in the MAIN window ─────
        # One click writes `editing = Title` and requests TITLE_EDITOR. The
        # node is painted only as a RESULT of that dispatch, so the request
        # misses and the re-derive is what makes it land. This branch worked
        # before R1463 — it is the twin the next one is measured against.
        tf.click(path=EDIT_TITLE)
        assert_eq(
            _focused(tf),
            TITLE_EDITOR,
            "the main window's just-appeared editor takes focus",
        )
        main = _snap(tf, MAIN)
        notes = _snap(tf, NOTES)
        assert _present(main, TITLE_EDITOR), "the title editor is painted in main"
        assert not _present(notes, TITLE_EDITOR), "and only in main"
        assert_eq(_status(main), "main: Editing the title (main window).", "status")
        assert_eq(
            _status(notes),
            "notes: Editing the title (main window).",
            "the state is binding-wide; each window paints its own copy",
        )
        assert TITLE_EDITOR in _tab_order(tf), "the editor joined the Tab order"

        # ── (D) the control's close path ────────────────────────────
        tf.key(path=EDIT_TITLE, name="Escape")
        assert_eq(
            _focused(tf),
            EDIT_TITLE,
            "closing names the trigger it came from — the same idiom",
        )
        assert not _present(_snap(tf, MAIN), TITLE_EDITOR), "the editor closed"
        assert_eq(_tab_order(tf), BASE_ORDER, "the base enumeration is back, whole")

        # ── (E) THE ROUND — the editor appears in the NOTES window ──
        # Byte-for-byte the same reducer shape as (C): set state, name a
        # tag. The binding does not know or care which window paints the
        # node. Pre-R1463 the retry re-derived the primary only, the notes
        # window answered from its harvested cache, and focus stayed on the
        # trigger this click had just given it.
        tf.click(path=EDIT_NOTE)
        focused = _focused(tf)
        assert focused != EDIT_NOTE, (
            "focus is still on the trigger: the request naming the notes "
            "editor was consumed by the miss and dropped. The named node is "
            "painted and focusable in a window that is on screen"
        )
        assert_eq(
            focused,
            NOTE_EDITOR,
            "the notes window's just-appeared editor takes focus, exactly as "
            "the main window's does",
        )

        notes = _snap(tf, NOTES)
        main = _snap(tf, MAIN)
        assert _present(notes, NOTE_EDITOR), "the note editor is painted in notes"
        assert not _present(main, NOTE_EDITOR), "and only there"
        assert not _present(main, TITLE_EDITOR), "the other editor stayed closed"
        assert_eq(
            _status(notes),
            "notes: Editing the note (notes window).",
            "the notes window reports the open editor",
        )
        assert_eq(
            _status(main),
            "main: Editing the note (notes window).",
            "the main window reports the same binding-wide state",
        )

        # ── (F) enumeration integrity ───────────────────────────────
        order = _tab_order(tf)
        assert_eq(
            order.count(NOTE_EDITOR),
            1,
            "the editor joins the union exactly ONCE — declared and painted "
            "windows are folded first-occurrence, not concatenated",
        )
        assert_eq(len(order), len(set(order)), f"no duplicate tags: {order!r}")
        assert order.index(EDIT_TITLE) < order.index(NOTES_PANE), (
            "the primary's tags come first"
        )
        for tag in BASE_ORDER:
            assert tag in order, f"{tag} survived the secondary-window refresh"
        assert not _refused(tf, NOTES_PANE), "the notes pane is still reachable"
        assert_eq(_focused(tf), NOTES_PANE, "focus/set moved within the notes window")
        assert not _refused(tf, NOTE_EDITOR), "and back to the editor"
        assert_eq(_focused(tf), NOTE_EDITOR, "focus is back on the editor")
        assert not _refused(tf, EDIT_TITLE), "a primary tag is reachable from there"
        assert_eq(
            tf.request("focus/next").result.get("focused"),
            EDIT_NOTE,
            "Tab walks the union in order: the primary's tags, then the rest",
        )

        # ── (G) close from the secondary window's editor ────────────
        tf.request("focus/set", {"tag": NOTE_EDITOR})
        tf.key(path=EDIT_NOTE, name="Escape")
        assert_eq(
            _focused(tf),
            EDIT_NOTE,
            "the close names its trigger, from the secondary window too",
        )
        notes = _snap(tf, NOTES)
        assert not _present(notes, NOTE_EDITOR), "the notes editor closed"
        assert _present(notes, NOTES_PANE), "its pane is untouched"
        assert_eq(_status(notes), "notes: No editor open.", "notes status reset")
        assert_eq(_status(_snap(tf, MAIN)), "main: No editor open.", "main reset")
        assert_eq(
            _tab_order(tf),
            BASE_ORDER,
            "the closed editor left the union with the paint — no stale tag "
            "stranded by the re-derive",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1463 §5.39 §5.16 — a request reaches a secondary window", body))
