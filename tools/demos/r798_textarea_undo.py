#!/usr/bin/env python3
"""R798 §5.52 — undo / redo in the multi-line rich-text textarea, over RPC.

R796 wired the `UndoStack` into `TextEditState` and hello-textfield was its
first consumer. R798 makes the **textarea** the 2nd `attach_undo` consumer —
the surface closest to the northern-star self-hosted editor (multi-line +
rich text). Because R796.1 made the command granular (it carries the deleted
`removed_runs`), undoing a deleted coloured word restores both its text *and*
its style run. An AI agent drives and reads the whole round-trip over the
§5.12 RPC plane (§2 #2), the same keys a human presses.

  (A) coalesced typing undoes / redoes as one word (Ctrl+Z / Ctrl+Shift+Z /
      Ctrl+Y), restoring the caret with it.
  (B) `Enter` inserts a newline (binding `apply_key`) and one Ctrl+Z removes
      it — undo spans the multi-line edit.
  (C) deleting the seeded red word "first" drops its style run; one Ctrl+Z
      restores the text AND the run (R796.1 run reversal, the headline).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r798_textarea_undo.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"
RED = (0xD0, 0x28, 0x28)


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def lines(ta: RpcSubprocess) -> int:
    return text(ta).count("\n") + 1


def type_text(ta: RpcSubprocess, s: str) -> None:
    for ch in s:
        ta.invoke("/external/key", "Space" if ch == " " else ch)


def chord(ta: RpcSubprocess, key: str, **mods: bool) -> bool:
    return ta.invoke("/external/key", {"key": key, **mods})


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    """The painted style run covering `byte`, or None (read from:paint)."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    assert field is not None, "textarea present in paint scene"
    texts = [n for n in _walk(field, []) if n.get("type") == "Text" and n.get("runs")]
    if not texts:
        return None
    for r in texts[0]["runs"]:
        if r["start"] <= byte < r["end"]:
            return r
    return None


def _ink(run: dict) -> tuple[int, int, int]:
    c = run["style"]["fg_color"]
    return (c["r"], c["g"], c["b"])


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        assert_eq(text(ta), SEED, "seeded multi-line text")
        assert_eq(lines(ta), 3, "three seeded lines")
        first = run_at(ta, 0)
        assert first is not None, "the leading word seeds a style run"
        assert_eq(_ink(first), RED, "'first' seeds red")
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="field focused")

        # ── (A) coalesced typing: one undo reverts the whole word ─────
        assert_eq(chord(ta, "End", ctrl=True), True, "Ctrl+End recognized")
        wait_query(ta, "/external/caret", len(SEED), desc="Ctrl+End moves to the document end")
        type_text(ta, "wow")
        wait_query(ta, "/external/text", SEED + "wow", desc="typing appended a word")
        assert_eq(caret(ta), len(SEED) + 3, "caret at the new end")

        assert_eq(chord(ta, "z", ctrl=True), True, "Ctrl+Z recognized")
        wait_query(
            ta, "/external/text", SEED,
            desc="one undo removed the whole 'wow' run (coalescing)",
        )
        assert_eq(caret(ta), len(SEED), "undo restored the caret with the text")
        # Cannot undo past the seeded document (the undo floor).
        # Floor no-op: the dispatch commits before the response.
        assert_eq(chord(ta, "z", ctrl=True), True, "Ctrl+Z consumed at the floor")
        assert_eq(text(ta), SEED, "no change below the seeded floor")

        # redo both ways: Ctrl+Shift+Z and Ctrl+Y rebuild the word.
        assert_eq(chord(ta, "z", ctrl=True, shift=True), True, "Ctrl+Shift+Z recognized")
        wait_query(ta, "/external/text", SEED + "wow", desc="Ctrl+Shift+Z redid the typing")
        chord(ta, "z", ctrl=True)
        wait_query(ta, "/external/text", SEED, desc="undo again before testing Ctrl+Y")
        assert_eq(chord(ta, "y", ctrl=True), True, "Ctrl+Y recognized")
        wait_query(ta, "/external/text", SEED + "wow", desc="Ctrl+Y is the alternate redo")
        chord(ta, "z", ctrl=True)
        wait_query(ta, "/external/text", SEED, desc="back to the floor for the next phase")

        # ── (B) Enter inserts a newline (binding) → one undo removes it ─
        # `Enter` is a binding-level key (multi-line newline), so it routes
        # through `scene/key` → apply_key, not the substrate key channel.
        ta.key(path=TA_TAG, name="Enter")
        wait_query(
            ta, "/external/text", SEED + "\n",
            desc="the newline landed at the document end",
        )
        assert_eq(lines(ta), 4, "Enter split the document into a 4th line")
        assert_eq(chord(ta, "z", ctrl=True), True, "Ctrl+Z recognized after Enter")
        wait_query(
            ta, "/external/text", SEED,
            desc="text back to the seed after the newline undo",
        )
        assert_eq(lines(ta), 3, "undo removed the inserted newline")

        # ── (C) run reversal: delete the red "first", undo restores it ─
        ta.intervene("/external/selection", {"start": 0, "end": 5})
        wait_query(ta, "/external/selection", {"start": 0, "end": 5}, desc="selected 'first'")
        assert _ink(run_at(ta, 0)) == RED, "'first' is red before the delete"

        assert_eq(chord(ta, "Backspace"), True, "Backspace deletes the selection")
        wait_query(
            ta, "/external/text", " line\nsecond line\nthird line",
            desc="the red word is gone",
        )
        assert run_at(ta, 0) is None, "its style run went with it"

        assert_eq(chord(ta, "z", ctrl=True), True, "Ctrl+Z recognized for the delete")
        wait_query(ta, "/external/text", SEED, desc="undo restored the deleted word")
        wait_until(
            lambda: run_at(ta, 0) is not None and _ink(run_at(ta, 0)) == RED,
            desc="undo restored the red style run (R796.1 removed_runs reversal)",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R798 §5.52 — textarea undo / redo + run reversal", body))
