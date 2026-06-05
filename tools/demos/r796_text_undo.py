#!/usr/bin/env python3
"""R796 §5.52 — text-field undo / redo with word-level coalescing, over RPC.

Every professional editor sits on a reversible-edit history; R748 built the
`UndoStack` substrate (the `QUndoStack` peer) but no text widget journalled to
it. R796 wires it into `TextEditState`: each content mutator records a
whole-content `TextEditCommand`, consecutive typing **coalesces** into one
undo step (a word, broken by whitespace — `QTextDocument`'s model), and
Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y) undo / redo. The history is AI-first: an agent
drives and reads the whole round-trip over the §5.12 RPC plane (§2 #2), the
same keys a human presses.

  (A) type two words → coalesced into two undo steps (not 11 keystrokes).
  (B) Ctrl+Z once → the second word vanishes as a unit (coalescing proof).
  (C) Ctrl+Z again → the first word + its trailing space vanish.
  (D) redo by Ctrl+Y and by Ctrl+Shift+Z → both rebuild the words.
  (E) caret is restored alongside text (whole-content snapshot).
  (F) a Backspace is a separate step from the typing run (group boundary).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

TF_TAG = "main_textfield"
PAUSE = 0.05


def type_text(tf: RpcSubprocess, s: str) -> None:
    for ch in s:
        tf.invoke("/external/key", "Space" if ch == " " else ch)
    time.sleep(PAUSE)


def chord(tf: RpcSubprocess, key: str, **mods: bool) -> bool:
    payload = {"key": key, **mods}
    out = tf.invoke("/external/key", payload)
    time.sleep(PAUSE)
    return out


def text(tf: RpcSubprocess) -> str:
    return tf.query("/external/text")


def caret(tf: RpcSubprocess) -> int:
    return tf.query("/external/caret")


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        tf.request("focus/set", {"tag": TF_TAG})
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/state"), "Focused", "field focused")

        # ── (A) type two words ──────────────────────────────────────
        type_text(tf, "hello world")
        assert_eq(text(tf), "hello world", "post-typing text")
        assert_eq(caret(tf), 11, "caret at end")

        # ── (B) one Ctrl+Z drops the whole second word (coalescing) ──
        assert_eq(chord(tf, "z", ctrl=True), True, "Ctrl+Z recognized")
        assert_eq(
            text(tf),
            "hello ",
            "one undo removed the entire 'world' run, not a single char",
        )
        assert_eq(caret(tf), 6, "caret restored to the word boundary")

        # ── (C) a second Ctrl+Z drops the first word + space ────────
        chord(tf, "z", ctrl=True)
        assert_eq(text(tf), "", "second undo cleared the first word + trailing space")
        assert_eq(caret(tf), 0, "caret back to the start")
        # Nothing left to undo.
        assert_eq(chord(tf, "z", ctrl=True), True, "Ctrl+Z consumed even at the bottom")
        assert_eq(text(tf), "", "no further change at the history bottom")

        # ── (D) redo via Ctrl+Y and via Ctrl+Shift+Z ────────────────
        assert_eq(chord(tf, "y", ctrl=True), True, "Ctrl+Y redo recognized")
        assert_eq(text(tf), "hello ", "Ctrl+Y re-applied the first step")
        assert_eq(chord(tf, "z", ctrl=True, shift=True), True, "Ctrl+Shift+Z recognized")
        assert_eq(text(tf), "hello world", "Ctrl+Shift+Z re-applied the second step")

        # ── (F) a Backspace is its own undo step ────────────────────
        chord(tf, "Backspace")
        assert_eq(text(tf), "hello worl", "backspace removed the last char")
        chord(tf, "z", ctrl=True)
        assert_eq(
            text(tf),
            "hello world",
            "undo restored the backspace alone (delete is a separate group)",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R796 §5.52 — text-field undo / redo coalescing", body))
