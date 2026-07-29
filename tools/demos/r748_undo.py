#!/usr/bin/env python3
"""R748 §5.52 — undo / redo command stack E2E.

Drives `hello-undo` via JSON-RPC. A counter editor sits on the `UndoStack`
substrate (the `QUndoStack` peer): the `+` / `-` buttons record reversible
`SignalEdit` commands; `Undo` / `Redo` step the cursor; the
`UndoStackExternal` at `undo_stack` surfaces the history as data.

The witness (§2 #7 scene-as-data):

  * (A) boot — value 0, empty history, both steps unavailable.
  * (B) edits record — `+` `+` advance the value AND the cursor; the history
    is two `Increment` commands; Undo available, Redo not.
  * (C) undo/redo via RPC (AI-first) — `invoke "undo"` reverts the value and
    steps the cursor back, enabling Redo; `invoke "redo"` re-applies.
  * (D) undo/redo via the buttons — clicking `Undo` / `Redo` drives the same
    stack (the reducer routes the click intent through it).
  * (E) heterogeneous label — `-` records a distinct `Decrement` command.
  * (F) redo-branch truncation — an edit after an undo drops the redo branch
    (the single-branch `QUndoStack` model).
  * (G) boundaries + clear — undo at the bottom is a no-op; `invoke "clear"`
    empties the history.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-undo"
WIN = (360, 280)
STACK = "/undo_stack/external"


def can_undo(d):
    return d.query(f"{STACK}/can_undo")


def can_redo(d):
    return d.query(f"{STACK}/can_redo")


def idx(d):
    return d.query(f"{STACK}/index")


def count(d):
    return d.query(f"{STACK}/count")


def find_text(node, content: str) -> bool:
    """Depth-first: is there a Text node whose content is exactly `content`?"""
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Text" and node.get("content") == content:
        return True
    for child in node.get("children") or []:
        if find_text(child, content):
            return True
    inner = node.get("content")
    if isinstance(inner, dict) and find_text(inner, content):
        return True
    return False


def value_is(tf, expected: int) -> None:
    """The big counter readout paints exactly `expected` (a lone integer
    Text node — the status line is a long string, never a bare number)."""
    snap = tf.snapshot(source="paint", viewport=WIN)
    assert find_text(snap, str(expected)), f"counter readout shows {expected}"


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: value 0, empty history, nothing to step ───────────
        rects = abs_rects_of(snap)
        for tag in ("inc", "dec", "undo", "redo"):
            assert tag in rects, f"{tag} button present at boot"
        assert_eq(can_undo(tf), False, "boot: cannot undo")
        assert_eq(can_redo(tf), False, "boot: cannot redo")
        assert_eq(idx(tf), 0, "boot cursor 0")
        assert_eq(count(tf), 0, "boot history empty")
        assert_eq(tf.query(f"{STACK}/undo_label"), None, "boot has no undo label")
        value_is(tf, 0)

        # ── (B) edits record onto the stack ─────────────────────────────
        tf.click(path="inc")
        wait_until(lambda: idx(tf) == 1, desc="first inc recorded")
        tf.click(path="inc")
        wait_until(lambda: idx(tf) == 2, desc="second inc recorded")
        assert_eq(count(tf), 2, "two commands recorded")
        assert_eq(can_undo(tf), True, "can undo after edits")
        assert_eq(can_redo(tf), False, "nothing to redo at the top")
        assert_eq(tf.query(f"{STACK}/undo_label"), "Increment", "next undo is an Increment")
        value_is(tf, 2)

        # ── (C) undo / redo via RPC (AI-first primary path) ─────────────
        assert_eq(tf.invoke(f"{STACK}/undo", None), True, "invoke undo stepped")
        assert_eq(idx(tf), 1, "cursor stepped back")
        assert_eq(can_redo(tf), True, "redo now available")
        assert_eq(tf.query(f"{STACK}/redo_label"), "Increment", "redo replays the Increment")
        value_is(tf, 1)
        assert_eq(tf.invoke(f"{STACK}/redo", None), True, "invoke redo stepped")
        assert_eq(idx(tf), 2, "cursor stepped forward")
        value_is(tf, 2)

        # ── (D) undo / redo via the buttons (reducer → same stack) ──────
        tf.click(path="undo")
        wait_until(lambda: idx(tf) == 1, desc="Undo button stepped back")
        value_is(tf, 1)
        tf.click(path="redo")
        wait_until(lambda: idx(tf) == 2, desc="Redo button stepped forward")
        value_is(tf, 2)

        # ── (E) decrement is a distinct, labelled, reversible edit ──────
        tf.click(path="dec")
        wait_until(lambda: count(tf) == 3, desc="decrement recorded")
        assert_eq(idx(tf), 3, "cursor at the new top")
        assert_eq(tf.query(f"{STACK}/undo_label"), "Decrement", "newest command is a Decrement")
        value_is(tf, 1)

        # ── (F) a new edit after an undo truncates the redo branch ──────
        assert_eq(tf.invoke(f"{STACK}/undo", None), True, "undo the decrement")
        assert_eq(can_redo(tf), True, "redo branch holds the decrement")
        value_is(tf, 2)
        tf.click(path="inc")
        wait_until(lambda: not can_redo(tf), desc="new edit dropped the redo branch")
        assert_eq(count(tf), 3, "history length is 3 (branch replaced, not appended)")
        assert_eq(tf.query(f"{STACK}/undo_label"), "Increment", "top command is the new Increment")
        value_is(tf, 3)

        # ── (G) boundary no-op + clear ──────────────────────────────────
        # Walk to the bottom, then one more undo is a no-op.
        while can_undo(tf):
            tf.invoke(f"{STACK}/undo", None)
        assert_eq(idx(tf), 0, "walked to the bottom")
        assert_eq(tf.invoke(f"{STACK}/undo", None), False, "undo at the bottom is a no-op")
        assert_eq(can_redo(tf), True, "everything is redoable from the bottom")
        assert_eq(tf.invoke(f"{STACK}/clear", None), 0, "clear returns cursor 0")
        assert_eq(count(tf), 0, "clear empties the history")
        assert_eq(can_undo(tf), False, "nothing to undo after clear")
        assert_eq(can_redo(tf), False, "nothing to redo after clear")


if __name__ == "__main__":
    sys.exit(run_demo("R748 §5.52 — undo/redo command stack", body))
