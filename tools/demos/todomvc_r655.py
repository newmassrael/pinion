#!/usr/bin/env python3
"""todomvc R655 §5.16 — first composed-app RPC self-verification demo.

R655 is the Phase-2 application-tier entry: the first `examples/*`
binding that composes **two** widgets in a single `WidgetView` (a
`TextField` for input + a dynamic `Vec<String>` rendered as
`Scene::Text` children). This demo drives the live binding entirely
over JSON-RPC 2.0 — no human-eye observation required — proving the
§2 invariant #2 ("RPC headless as AI primary path") and §2 invariant
#7 ("scene-as-data") apply to composed apps the same way they do to
single-widget gallery bindings.

Driven sequence (each step ends in a typed assertion):

  1. focus/set TF_TAG                         → field receives focus
  2. scene/invoke "/external/key" 'm','i','l','k'  → textfield text="milk"
  3. scene/key path=TF_TAG name="Enter"       → V::apply_key intercepts,
                                                  appends to use_todos
                                                  signal, clears field
  4. scene/snapshot source="paint"            → walk scene → LIST_TAG
                                                  has header + 1 item
  5. repeat (1)-(3) for "eggs"                → list has 2 items
  6. trim guard: type "   " (whitespace) + Enter → list unchanged
  7. unicode: type "✓" + Enter                → list grows to 3 items
                                                  with the unicode entry

The trimmed-whitespace case (step 6) exercises the
`!text.trim().is_empty()` guard in `WidgetCore::apply_key` (mirrors
the TasteJS TodoMVC spec: blank submissions are silently discarded).

The demo also exercises `scene/query "/external/state"` between
steps to confirm the SCXML textfield stays in `Focused` posture
across the Enter submit / clear / continue-typing cycle.

Phase-2 cascade — R656 will extend this demo with per-item
toggle / edit / delete invocations once those routes land; R657
adds filter introspection; R658 verifies persistence round-trip
across a process restart.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo

TF_TAG = "main_textfield"
LIST_TAG = "todo_list"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text(tf: RpcSubprocess, text: str) -> None:
    """Type `text` character-by-character through the textfield's
    `invoke("key", Text)` channel — same path the platform keyboard
    arc uses, just driven by RPC."""
    for ch in text:
        result = tf.invoke("/external/key", ch)
        # The substrate returns Bool(true) for any recognized key; a
        # `false` here means the textfield rejected the character
        # (only happens for unrecognized W3C key names, not single
        # printable chars).
        assert_eq(result, True, f"invoke('key', {ch!r}) recognized")
    time.sleep(0.05)


def submit_enter(tf: RpcSubprocess) -> None:
    """Press Enter at the textfield's rect centre. Routes through
    `scene/key` → shell `handle_named_key` → `V::apply_key`, where the
    todomvc binding intercepts Enter BEFORE delegating to the
    textfield's invoke channel."""
    tf.key(path=TF_TAG, name="Enter")
    time.sleep(0.1)


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    """Walk the snapshot JSON tree depth-first for the first node
    carrying `tag`. Returns the node dict or None."""
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        found = find_node_by_tag(child, tag)
        if found is not None:
            return found
    # Scroll wrappers carry their child under `content`, not `children`.
    content = node.get("content")
    if isinstance(content, dict):
        found = find_node_by_tag(content, tag)
        if found is not None:
            return found
    return None


def list_entry_count(tf: RpcSubprocess) -> int:
    """Snapshot the paint scene + walk to the LIST_TAG container,
    return (child_count - 1) so the placeholder header is excluded
    from the entry count."""
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, f"snapshot must carry {LIST_TAG} tag"
    children = list_node.get("children") or []
    # Header + N items; the header is always the first child.
    return max(0, len(children) - 1)


def list_entry_texts(tf: RpcSubprocess) -> list[str]:
    """Walk the LIST_TAG container's children and collect the text
    content of each entry row. The header (first child) is skipped."""
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, f"snapshot must carry {LIST_TAG} tag"
    children = list_node.get("children") or []
    out: list[str] = []
    # Skip index 0 (placeholder / "Todos (N)" header).
    for row in children[1:]:
        # Each row is a Container holding a single Scene::Text node.
        row_kids = row.get("children") or []
        for kid in row_kids:
            if kid.get("type") == "Text":
                content = kid.get("content")
                if isinstance(content, str):
                    out.append(content)
    return out


def body() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (0) Initial posture ────────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "initial state")
        assert_eq(tf.query("/external/text"), "", "initial text")
        assert_eq(list_entry_count(tf), 0, "initial list is empty")

        # ── (1) Focus the textfield via focus mgr ─────────────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        assert_eq(
            tf.query("/external/state"),
            "Focused",
            "post-focus state",
        )

        # ── (2) Type 'milk' + Enter ───────────────────────────────
        type_text(tf, "milk")
        assert_eq(tf.query("/external/text"), "milk", "typed 'milk'")
        submit_enter(tf)
        assert_eq(
            tf.query("/external/text"),
            "",
            "Enter clears the textfield",
        )
        assert_eq(
            tf.query("/external/caret"),
            0,
            "Enter resets caret to 0",
        )
        assert_eq(
            tf.query("/external/state"),
            "Focused",
            "Enter preserves focus posture",
        )
        assert_eq(list_entry_count(tf), 1, "list grew to 1 entry")
        assert_eq(
            list_entry_texts(tf),
            ["milk"],
            "first entry is 'milk'",
        )

        # ── (3) Type 'eggs' + Enter ───────────────────────────────
        type_text(tf, "eggs")
        assert_eq(tf.query("/external/text"), "eggs", "typed 'eggs'")
        submit_enter(tf)
        assert_eq(list_entry_count(tf), 2, "list grew to 2 entries")
        assert_eq(
            list_entry_texts(tf),
            ["milk", "eggs"],
            "submission order preserved",
        )

        # ── (4) Whitespace trim guard ─────────────────────────────
        type_text(tf, "   ")
        assert_eq(
            tf.query("/external/text"),
            "   ",
            "three spaces typed",
        )
        submit_enter(tf)
        assert_eq(
            list_entry_count(tf),
            2,
            "blank-after-trim Enter is silently discarded",
        )
        # The trim guard does NOT clear the field (the early-return
        # is on the "no submit" path; the field keeps the whitespace
        # so the user can edit instead of re-typing). This is the
        # textbook TasteJS TodoMVC behaviour.
        assert_eq(
            tf.query("/external/text"),
            "   ",
            "whitespace text preserved after blank-discard",
        )

        # Manually clear the field via the textfield's invoke channel
        # before continuing — three backspaces unwind the spaces.
        for _ in range(3):
            tf.invoke("/external/key", "Backspace")
        time.sleep(0.05)
        assert_eq(
            tf.query("/external/text"),
            "",
            "field cleared via Backspace",
        )

        # ── (5) Unicode entry ─────────────────────────────────────
        type_text(tf, "✓")
        assert_eq(
            tf.query("/external/text"),
            "✓",
            "unicode glyph typed",
        )
        submit_enter(tf)
        assert_eq(list_entry_count(tf), 3, "unicode entry appended")
        assert_eq(
            list_entry_texts(tf),
            ["milk", "eggs", "✓"],
            "unicode entry preserved in list",
        )


if __name__ == "__main__":
    sys.exit(run_demo("todomvc R655", body))
