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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_query


TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        run_body(tf)


def run_body(tf: RpcSubprocess) -> None:
    focus_set(tf, TF_TAG)
    wait_query(tf, "/external/state", "Focused", desc="post-focus state")

    # Seed the buffer with "hello world".
    for ch in "hello world":
        tf.invoke("/external/key", "Space" if ch == " " else ch)
    wait_query(tf, "/external/text", "hello world", desc="post-typing text")

    # ── Ctrl+C: select first 5 chars then copy.
    tf.intervene("/external/selection", {"start": 0, "end": 5})
    wait_query(
        tf, "/external/selection", {"start": 0, "end": 5},
        desc="selection set via intervene",
    )
    assert_eq(
        tf.invoke("/external/key", {"key": "c", "ctrl": True}),
        True,
        "Ctrl+C recognized",
    )
    # Selection survives copy (canonical UX).
    wait_query(
        tf, "/external/selection", {"start": 0, "end": 5},
        desc="selection survives copy",
    )
    # Text unchanged.
    assert_eq(tf.query("/external/text"), "hello world", "text unchanged by copy")

    # ── Ctrl+V at end: caret to end, clear selection, paste "hello".
    tf.invoke("/external/key", "End")
    wait_query(tf, "/external/caret", 11, desc="caret at end")
    assert_eq(tf.query("/external/selection"), None, "no selection after End")
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V recognized",
    )
    wait_query(
        tf, "/external/text", "hello worldhello",
        desc="Ctrl+V inserted clipboard at caret",
    )
    assert_eq(tf.query("/external/caret"), 16, "caret advanced past pasted text")

    # ── Ctrl+X: select last 5 chars and cut.
    tf.intervene("/external/selection", {"start": 11, "end": 16})
    wait_query(
        tf, "/external/selection", {"start": 11, "end": 16},
        desc="cut selection set via intervene",
    )
    assert_eq(
        tf.invoke("/external/key", {"key": "x", "ctrl": True}),
        True,
        "Ctrl+X recognized",
    )
    wait_query(
        tf, "/external/text", "hello world",
        desc="cut drained selection from text",
    )
    assert_eq(tf.query("/external/selection"), None, "selection cleared after cut")
    assert_eq(tf.query("/external/caret"), 11, "caret at cut start")

    # ── Ctrl+V again: round-trip the cut text back.
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V after cut",
    )
    wait_query(
        tf, "/external/text", "hello worldhello",
        desc="Ctrl+V after Ctrl+X round-trips the payload",
    )

    # ── Plain 'c' / 'x' / 'v' (no modifier) still insert literals.
    # Clear the buffer first via select-all + backspace.
    tf.invoke("/external/key", {"key": "a", "ctrl": True})
    tf.invoke("/external/key", "Backspace")
    wait_query(
        tf, "/external/text", "",
        desc="buffer cleared via select-all + backspace",
    )
    for ch in ["c", "x", "v"]:
        assert_eq(
            tf.invoke("/external/key", ch),
            True,
            f"plain {ch!r} inserts literally",
        )
    wait_query(tf, "/external/text", "cxv", desc="plain c/x/v typed")

    # ── Ctrl+C / Ctrl+V with an existing payload: paste replaces
    # selection (R56.1.f.1 type-to-replace through paste path).
    tf.intervene("/external/selection", {"start": 0, "end": 3})
    wait_query(
        tf, "/external/selection", {"start": 0, "end": 3},
        desc="paste-target selection set via intervene",
    )
    # Clipboard still holds "hello" from the round-trip above.
    assert_eq(
        tf.invoke("/external/key", {"key": "v", "ctrl": True}),
        True,
        "Ctrl+V on active selection",
    )
    wait_query(
        tf, "/external/text", "hello",
        desc="Ctrl+V replaced 'cxv' selection with clipboard 'hello'",
    )

    focus_set(tf, None)


if __name__ == "__main__":
    sys.exit(run_demo("hello-textfield clipboard", body))
