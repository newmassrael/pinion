#!/usr/bin/env python3
"""hello-textfield clipboard dogfood (R56.1.e §5.22 §5.49).

End-to-end RPC self-verify for the R56.1.e clipboard cascade:

- Ctrl+C copy: selection contents flow into the attached
  InMemoryClipboard (verified indirectly by Ctrl+V round-trip).
- Ctrl+X cut: selection drained from the text + clipboard payload
  populated; subsequent Ctrl+V reproduces the cut text.
- Ctrl+V paste at caret: in-memory clipboard payload inserts at the
  caret position; if a selection is active, the range is replaced
  (R56.1.f.1 selection-aware insert).
- Plain 'c' / 'x' / 'v' (no modifier) still type the literal letter
  — the printable-char path stays intact.

Drives the same hello-textfield binary the prior R56.1.b.1 / f.3
demos use; the binding's `create_external` attaches a fresh
InMemoryClipboard via the `use_clipboard` hook so RPC can drive the
keystroke dispatcher end-to-end without OS clipboard plumbing.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        run_body(tf)


def run_body(tf: RpcSubprocess) -> None:
    focus_set(tf, TF_TAG)
    time.sleep(0.05)
    assert_eq(tf.query("/external/state"), "Focused", "post-focus state")

    # Seed the buffer with "hello world".
    for ch in "hello world":
        tf.invoke("/external/key", "Space" if ch == " " else ch)
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "hello world", "post-typing text")

    # ── Ctrl+C: select first 5 chars then copy.
    tf.intervene("/external/selection", {"start": 0, "end": 5})
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/selection"),
        {"start": 0, "end": 5},
        "selection set via intervene",
    )
    assert_eq(
        tf.invoke("/external/key", {"key": "c", "ctrl": True}),
        True,
        "Ctrl+C recognized",
    )
    time.sleep(0.05)
    # Selection survives copy (canonical UX).
    assert_eq(
        tf.query("/external/selection"),
        {"start": 0, "end": 5},
        "selection survives copy",
    )
    # Text unchanged.
    assert_eq(tf.query("/external/text"), "hello world", "text unchanged by copy")

    # ── Ctrl+V at end: caret to end, clear selection, paste "hello".
    tf.invoke("/external/key", "End")
    time.sleep(0.05)
    assert_eq(tf.query("/external/caret"), 11, "caret at end")
    assert_eq(tf.query("/external/selection"), None, "no selection after End")
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V recognized",
    )
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/text"),
        "hello worldhello",
        "Ctrl+V inserted clipboard at caret",
    )
    assert_eq(tf.query("/external/caret"), 16, "caret advanced past pasted text")

    # ── Ctrl+X: select last 5 chars and cut.
    tf.intervene("/external/selection", {"start": 11, "end": 16})
    time.sleep(0.05)
    assert_eq(
        tf.invoke("/external/key", {"key": "x", "ctrl": True}),
        True,
        "Ctrl+X recognized",
    )
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "hello world", "cut drained selection from text")
    assert_eq(tf.query("/external/selection"), None, "selection cleared after cut")
    assert_eq(tf.query("/external/caret"), 11, "caret at cut start")

    # ── Ctrl+V again: round-trip the cut text back.
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V after cut",
    )
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/text"),
        "hello worldhello",
        "Ctrl+V after Ctrl+X round-trips the payload",
    )

    # ── Plain 'c' / 'x' / 'v' (no modifier) still insert literals.
    # Clear the buffer first via select-all + backspace.
    tf.invoke("/external/key", {"key": "a", "ctrl": True})
    tf.invoke("/external/key", "Backspace")
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "", "buffer cleared via select-all + backspace")
    for ch in ["c", "x", "v"]:
        assert_eq(
            tf.invoke("/external/key", ch),
            True,
            f"plain {ch!r} inserts literally",
        )
    time.sleep(0.05)
    assert_eq(tf.query("/external/text"), "cxv", "plain c/x/v typed")

    # ── Ctrl+C / Ctrl+V with an existing payload: paste replaces
    # selection (R56.1.f.1 type-to-replace through paste path).
    tf.intervene("/external/selection", {"start": 0, "end": 3})
    time.sleep(0.05)
    # Clipboard still holds "hello" from the round-trip above.
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V on active selection",
    )
    time.sleep(0.05)
    assert_eq(
        tf.query("/external/text"),
        "hello",
        "Ctrl+V replaced 'cxv' selection with clipboard 'hello'",
    )

    focus_set(tf, None)


if __name__ == "__main__":
    sys.exit(run_demo("hello-textfield clipboard", body))
