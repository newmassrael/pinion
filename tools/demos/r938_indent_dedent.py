#!/usr/bin/env python3
"""R938 §5.22 — Tab / Shift+Tab indent & dedent on a multi-line code editor.

The depth slice that turns the syntax-highlight field into a usable code
editor: `Tab` indents the selected lines, `Shift+Tab` dedents them, and the
AI-first `indent` / `dedent` invoke verbs drive the same edit over RPC. A
multi-line indent groups its per-line splices into ONE undo step (the
`UndoStack` macro transaction, 2nd consumer after replace-all), and — the
R933.1 discipline — each per-line splice shifts the style runs AND the fold
anchors, so a collapsed fold inside an indented block survives.

This demo drives it over RPC and verifies, all without OCR:

  (A) AI-first `indent` / `dedent` verbs round-trip a multi-line selection,
      each returning whether the buffer changed (setter-returns-read-outcome).
  (B) A dedent with no leading whitespace is a Bool(false) no-op.
  (C) A single-line / collapsed indent inserts the unit at the caret.
  (D) The keyboard twin — `scene/key` "Tab" / Shift+"Tab" — lands the same
      edit (the field reports the key handled, so the shell does NOT traverse
      focus away from the editor).
  (E) A block indent is ONE undo step: Ctrl+Z reverses the whole block,
      Ctrl+Shift+Z re-applies it.
  (F) The R933.1 headline — indenting a block that contains a COLLAPSED fold
      keeps the fold collapsed (the anchor shifted, never clipped).

Text glyph rasterisation itself is covered by r904_syntax_highlight.py; this
round adds no new paint, so there is no separate live-pixel guard.

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r938_indent_dedent.py
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
        # ── (A) AI-first indent / dedent round-trips a multi-line selection ──
        _set_text(ed, "a\nb\nc")
        _select(ed, 0, 5)
        assert_eq(ed.invoke(f"{_EXT}/indent", None), True, "indent reports a change")
        wait_until(lambda: _text(ed) == "    a\n    b\n    c", desc="every line gained a unit")
        # The selection re-covers the indented block (first line start -> shifted end).
        assert_eq(ed.query(f"{_EXT}/selection"), {"start": 0, "end": 17}, "block re-selected")
        assert_eq(ed.invoke(f"{_EXT}/dedent", None), True, "dedent reports a change")
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="dedent restores the un-indented lines")

        # ── (B) dedent with no leading whitespace is a no-op ────────────────
        _select(ed, 0, 5)
        assert_eq(ed.invoke(f"{_EXT}/dedent", None), False, "nothing to strip -> Bool(false)")
        assert_eq(_text(ed), "a\nb\nc", "a no-op dedent leaves the buffer untouched")

        # ── (C) single-line / collapsed indent inserts the unit at the caret ─
        _set_text(ed, "xy")
        ed.intervene(f"{_EXT}/selection", None)  # collapse
        ed.intervene(f"{_EXT}/caret", 1)
        wait_until(lambda: ed.query(f"{_EXT}/caret") == 1, desc="caret parked mid-line")
        assert_eq(ed.invoke(f"{_EXT}/indent", None), True, "indent at caret reports a change")
        wait_until(lambda: _text(ed) == "x    y", desc="the unit lands at the caret, not line start")
        assert_eq(ed.query(f"{_EXT}/caret"), 5, "caret follows past the inserted unit")

        # ── (D) the keyboard twin: Tab / Shift+Tab land the same edit ───────
        _set_text(ed, "p\nq")
        _select(ed, 0, 3)
        assert_eq(_key(ed, "Tab"), True, "Tab is handled by the editor (no focus traversal)")
        wait_until(lambda: _text(ed) == "    p\n    q", desc="Tab indented the block")
        _select(ed, 0, len("    p\n    q"))
        assert_eq(_key(ed, "Tab", shift=True), True, "Shift+Tab is handled by the editor")
        wait_until(lambda: _text(ed) == "p\nq", desc="Shift+Tab dedented the block")

        # ── (E) a block indent is ONE undo step ─────────────────────────────
        _set_text(ed, "m\nn")
        _select(ed, 0, 3)
        ed.invoke(f"{_EXT}/indent", None)
        wait_until(lambda: _text(ed) == "    m\n    n", desc="block indented before undo")
        assert_eq(_key(ed, "z", ctrl=True), True, "Ctrl+Z handled")
        wait_until(lambda: _text(ed) == "m\nn", desc="one undo reverses the whole block indent")
        assert_eq(_key(ed, "z", ctrl=True, shift=True), True, "Ctrl+Shift+Z handled")
        wait_until(lambda: _text(ed) == "    m\n    n", desc="one redo re-applies the block indent")

        # ── (F) R933.1 — indenting a block keeps an interior fold collapsed ──
        _set_text(ed, "fn a() {\n  x\n}\nz")
        assert_eq(ed.invoke(f"{_EXT}/toggle-fold", 0), True, "collapse the function body")
        wait_until(lambda: _collapsed_openers(ed) == {0}, desc="opener 0 collapsed")
        _select(ed, 0, len("fn a() {\n  x\n}\nz"))
        assert_eq(ed.invoke(f"{_EXT}/indent", None), True, "indent the folded block")
        wait_until(
            lambda: _text(ed) == "    fn a() {\n      x\n    }\n    z",
            desc="block indented around the fold",
        )
        # The fold anchor shifted with its '{' — it did NOT spring open.
        assert_eq(_collapsed_openers(ed), {0}, "the interior fold survived the indent")
        # And one undo restores both the text and the fold.
        assert_eq(_key(ed, "z", ctrl=True), True, "Ctrl+Z after the folded indent")
        wait_until(lambda: _text(ed) == "fn a() {\n  x\n}\nz", desc="text restored")
        assert_eq(_collapsed_openers(ed), {0}, "fold still valid after undo")


if __name__ == "__main__":
    sys.exit(run_demo("R938 indent / dedent", body))
