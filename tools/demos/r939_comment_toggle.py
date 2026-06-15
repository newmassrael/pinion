#!/usr/bin/env python3
"""R939 §5.22 — Ctrl+/ toggle line comments on a multi-line code editor.

The next depth slice of the syntax-highlight code editor and the third member
of the line-operation family (indent / dedent / comment-toggle): `Ctrl+/`
comments the selected lines, toggles them back when they are all commented,
and the AI-first `toggle-comment` invoke verb drives the same edit over RPC.
The touched lines come from `line_starts_in_range` (the line-op SSOT's 2nd
consumer), the per-line splices group into ONE undo step (the `UndoStack`
macro transaction, 3rd consumer after replace-all and indent / dedent), and —
the R933.1 discipline — each splice shifts the style runs AND the fold anchors,
so a collapsed fold inside a commented block survives.

This demo drives it over RPC and verifies, all without OCR:

  (A) AI-first `toggle-comment` round-trips a multi-line selection (comment
      all -> uncomment all), each returning whether the buffer changed
      (setter-returns-read-outcome), and re-covers the block selection.
  (B) A toggle over only blank lines is a Bool(false) no-op.
  (C) The keyboard twin — `scene/key` Ctrl+"/" — lands the same edit, while a
      plain "/" stays a literal insert (the Ctrl gate).
  (D) A partial block (some lines commented, some not) comments ALL lines (the
      VS Code "Toggle Line Comment" verdict), preserving each line's indent.
  (E) A block toggle is ONE undo step: Ctrl+Z reverses the whole block,
      Ctrl+Shift+Z re-applies it.
  (F) The R933.1 headline — toggling comments on a block that contains a
      COLLAPSED fold keeps the fold collapsed (the anchor shifted, never
      clipped).

Text glyph rasterisation itself is covered by r904_syntax_highlight.py; this
round adds no new paint, so there is no separate live-pixel guard.

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r939_comment_toggle.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

# hello-syntax-highlight's TF_TAG; the single primary External answers at /external.
_EXT = "/external"


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{_EXT}/text", text)
    wait_until(lambda: ed.query(f"{_EXT}/text") == text, desc=f"buffer becomes {text!r}")


def _select(ed: RpcSubprocess, start: int, end: int) -> None:
    ed.intervene(f"{_EXT}/selection", {"start": start, "end": end})
    wait_until(
        lambda: ed.query(f"{_EXT}/selection") == {"start": start, "end": end},
        desc=f"selection == ({start}, {end})",
    )


def _text(ed: RpcSubprocess) -> str:
    return ed.query(f"{_EXT}/text")


def _key(ed: RpcSubprocess, key: str, *, shift: bool = False, ctrl: bool = False) -> Any:
    return ed.invoke(f"{_EXT}/key", {"key": key, "shift": shift, "ctrl": ctrl})


def _collapsed_openers(ed: RpcSubprocess) -> set[int]:
    regions = ed.query(f"{_EXT}/fold_regions")
    assert isinstance(regions, list), f"fold_regions must be a JSON array, got {regions!r}"
    return {r["start_line"] for r in regions if r["collapsed"]}


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── (A) AI-first toggle-comment round-trips a multi-line selection ───
        _set_text(ed, "a\nb\nc")
        _select(ed, 0, 5)
        assert_eq(ed.invoke(f"{_EXT}/toggle-comment", None), True, "comment reports a change")
        wait_until(lambda: _text(ed) == "// a\n// b\n// c", desc="every line gained a marker")
        # The selection re-covers the commented block (first line start -> shifted end).
        assert_eq(ed.query(f"{_EXT}/selection"), {"start": 0, "end": 14}, "block re-selected")
        assert_eq(ed.invoke(f"{_EXT}/toggle-comment", None), True, "uncomment reports a change")
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="second toggle uncomments the block")

        # ── (B) a toggle over only blank lines is a no-op ───────────────────
        _set_text(ed, "  \n  ")
        _select(ed, 0, 5)
        assert_eq(ed.invoke(f"{_EXT}/toggle-comment", None), False, "blank lines -> Bool(false)")
        assert_eq(_text(ed), "  \n  ", "a no-op toggle leaves the buffer untouched")

        # ── (C) the keyboard twin: Ctrl+/ lands the same edit ───────────────
        _set_text(ed, "ab")
        _select(ed, 0, 2)
        assert_eq(_key(ed, "/", ctrl=True), True, "Ctrl+/ is handled by the editor")
        wait_until(lambda: _text(ed) == "// ab", desc="Ctrl+/ commented the line")
        _select(ed, 0, len("// ab"))
        assert_eq(_key(ed, "/", ctrl=True), True, "Ctrl+/ again is handled")
        wait_until(lambda: _text(ed) == "ab", desc="Ctrl+/ uncommented the line")
        # A plain "/" (no Ctrl) is still a literal insert — the Ctrl gate.
        ed.intervene(f"{_EXT}/selection", None)  # collapse
        ed.intervene(f"{_EXT}/caret", 2)
        wait_until(lambda: ed.query(f"{_EXT}/caret") == 2, desc="caret at end")
        assert_eq(_key(ed, "/"), True, "plain / is a printable insert")
        wait_until(lambda: _text(ed) == "ab/", desc="a bare slash inserts a literal")

        # ── (D) a partial block comments ALL lines, preserving indent ───────
        _set_text(ed, "// a\n  b")
        _select(ed, 0, len("// a\n  b"))
        assert_eq(ed.invoke(f"{_EXT}/toggle-comment", None), True, "partial block -> add to all")
        wait_until(
            lambda: _text(ed) == "// // a\n  // b",
            desc="the uncommented line gains a marker, the indent hugs the code",
        )

        # ── (E) a block toggle is ONE undo step ─────────────────────────────
        _set_text(ed, "m\nn")
        _select(ed, 0, 3)
        ed.invoke(f"{_EXT}/toggle-comment", None)
        wait_until(lambda: _text(ed) == "// m\n// n", desc="block commented before undo")
        assert_eq(_key(ed, "z", ctrl=True), True, "Ctrl+Z handled")
        wait_until(lambda: _text(ed) == "m\nn", desc="one undo reverses the whole block comment")
        assert_eq(_key(ed, "z", ctrl=True, shift=True), True, "Ctrl+Shift+Z handled")
        wait_until(lambda: _text(ed) == "// m\n// n", desc="one redo re-applies the block comment")

        # ── (F) R933.1 — commenting a block keeps an interior fold collapsed ─
        _set_text(ed, "fn a() {\n  x\n}\nz")
        assert_eq(ed.invoke(f"{_EXT}/toggle-fold", 0), True, "collapse the function body")
        wait_until(lambda: _collapsed_openers(ed) == {0}, desc="opener 0 collapsed")
        _select(ed, 0, len("fn a() {\n  x\n}\nz"))
        assert_eq(ed.invoke(f"{_EXT}/toggle-comment", None), True, "comment the folded block")
        wait_until(
            lambda: _text(ed) == "// fn a() {\n  // x\n// }\n// z",
            desc="block commented around the fold",
        )
        # The fold anchor shifted with its '{' — it did NOT spring open.
        assert_eq(_collapsed_openers(ed), {0}, "the interior fold survived the comment toggle")
        # And one undo restores both the text and the fold.
        assert_eq(_key(ed, "z", ctrl=True), True, "Ctrl+Z after the folded comment")
        wait_until(lambda: _text(ed) == "fn a() {\n  x\n}\nz", desc="text restored")
        assert_eq(_collapsed_openers(ed), {0}, "fold still valid after undo")


if __name__ == "__main__":
    sys.exit(run_demo("R939 comment toggle", body))
