#!/usr/bin/env python3
"""hello-textfield selection dogfood (R56.1.f.3 §5.22 §5.38 §5.49).

End-to-end RPC self-verify for the R56.1.f text-selection cascade:

- Shift+Arrow extension via the modifier-aware
  invoke("key", {"key": ..., "shift": true}) Json shape (R56.1.f.0).
- selection_range introspect via query("/external/selection")
  returning Json {"start": int, "end": int} or null when collapsed.
- intervene("/external/selection", {"start": ..., "end": ...}) sets
  both ends atomically (R56.1.f.3 substrate).
- type-to-replace: a printable keystroke with an active selection
  drains the selected range and inserts the new text in place
  (R56.1.f.1 selection-aware insert / backspace / delete_forward).
- Ctrl+A select-all via the Json modifier path (R56.1.f.2 +
  R56.1.f.0).

The demo drives the same hello-textfield binary the R56.1.b.1
visible-consumer demo uses; no rebuild is needed across rounds.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    """`focus/set` wrapper mirroring hello_textfield_type.py."""
    tf.request("focus/set", {"tag": tag})


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        run_body(tf)


def run_body(tf: RpcSubprocess) -> None:
    # Pre-flight: schema includes the R56.1.f.3 `selection` slot.
    assert_eq(tf.query("/external/state"), "Idle", "initial /external/state")
    assert_eq(
        tf.query("/external/selection"),
        None,
        "initial /external/selection is null (collapsed)",
    )

    # Focus the field so subsequent apply_key dispatch resolves the
    # field as the focused widget.
    focus_set(tf, TF_TAG)
    time.sleep(0.05)
    assert_eq(tf.query("/external/state"), "Focused", "post-focus state")

    # Type "hello world" verbatim.
    for ch in "hello world":
        # Space arrives as the canonical W3C named-key string.
        key = "Space" if ch == " " else ch
        assert_eq(
            tf.invoke("/external/key", key),
            True,
            f"invoke('key', {key!r}) recognized",
        )
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "hello world", "post-typing text")
    assert_eq(tf.query("/external/caret"), 11, "post-typing caret at end")
    assert_eq(
        tf.query("/external/selection"),
        None,
        "no selection after plain typing",
    )

    # ── R56.1.f.0 + R56.1.f.2: Shift+ArrowLeft extends selection.
    # Json invoke args shape carries the shift bit; selection_range
    # surfaces a fresh range with start = caret - 1.
    for expected_end in (10, 9, 8, 7, 6):
        assert_eq(
            tf.invoke("/external/key", {"key": "ArrowLeft", "shift": True}),
            True,
            f"Shift+ArrowLeft towards {expected_end}",
        )
    time.sleep(0.05)
    assert_eq(tf.query("/external/caret"), 6, "after 5x Shift+ArrowLeft caret")
    sel = tf.query("/external/selection")
    assert_eq(sel, {"start": 6, "end": 11}, "selection range after Shift+ArrowLeft")

    # ── R56.1.f.1: plain ArrowLeft collapses selection to start.
    assert_eq(
        tf.invoke("/external/key", "ArrowLeft"),
        True,
        "plain ArrowLeft on a selection collapses",
    )
    time.sleep(0.05)
    assert_eq(tf.query("/external/caret"), 6, "ArrowLeft lands at selection start")
    assert_eq(
        tf.query("/external/selection"),
        None,
        "selection collapses after plain ArrowLeft",
    )

    # ── R56.1.f.3: intervene("selection", Json) sets both ends.
    tf.intervene("/external/selection", {"start": 0, "end": 5})
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/selection"),
        {"start": 0, "end": 5},
        "intervene selection round-trip",
    )

    # ── R56.1.f.1 type-to-replace: a printable keystroke with active
    # selection replaces the selected range. "hello" (0..5) becomes
    # "Hi", caret lands at 2.
    assert_eq(
        tf.invoke("/external/key", "H"),
        True,
        "type 'H' replaces selection start",
    )
    assert_eq(
        tf.invoke("/external/key", "i"),
        True,
        "type 'i' continues after replacement",
    )
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "Hi world", "type-to-replace produced 'Hi world'")
    assert_eq(tf.query("/external/caret"), 2, "caret at end of replacement")
    assert_eq(
        tf.query("/external/selection"),
        None,
        "type-to-replace collapses selection",
    )

    # ── R56.1.f.2 + R56.1.f.0: Ctrl+A selects entire text via Json.
    assert_eq(
        tf.invoke("/external/key", {"key": "a", "ctrl": True}),
        True,
        "Ctrl+A select-all recognized",
    )
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/selection"),
        {"start": 0, "end": 8},
        "Ctrl+A selects entire 'Hi world'",
    )
    assert_eq(tf.query("/external/caret"), 8, "Ctrl+A leaves caret at end")

    # Backspace on a full selection drains the buffer entirely.
    assert_eq(
        tf.invoke("/external/key", "Backspace"),
        True,
        "Backspace on full selection",
    )
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "", "buffer cleared")
    assert_eq(tf.query("/external/caret"), 0, "caret at zero")
    assert_eq(tf.query("/external/selection"), None, "no selection post-drain")

    # ── intervene("selection", null) is a no-op idempotent on the
    # already-collapsed caret — the wire shape must still be accepted.
    tf.intervene("/external/selection", None)
    time.sleep(0.05)
    assert_eq(tf.query("/external/selection"), None, "null intervene keeps collapsed")

    # Blur — selection state persists in the underlying TextEditState
    # but the SCXML reports Idle.
    focus_set(tf, None)
    time.sleep(0.05)
    assert_eq(tf.query("/external/state"), "Idle", "post-blur state")


if __name__ == "__main__":
    sys.exit(run_demo("hello-textfield selection", body))
