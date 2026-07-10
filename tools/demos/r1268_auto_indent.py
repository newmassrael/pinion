#!/usr/bin/env python3
"""R1268 §5.22 — auto-indent on Enter in a multi-line code editor.

The depth slice that turns the syntax-highlight field from a viewer into a
usable editor: pressing Enter inserts a newline that COPIES the current line's
leading indentation (the code-editor "keep indentation" affordance), so adding
a line inside an indented block stays indented. Pre-R1268 Enter reached the
shared field keymap and was a no-op here (Enter is neither a printable char nor
a handled named key), so the editor could not even add a line.

The behaviour lives on the `TextEditState` newline SSOT
(`insert_newline`) behind the opt-in `set_auto_indent(true)`, gated in the
shared field keymap next to Tab-indent / Ctrl+/-comment; a single-line field or
a prose textarea leaves it off and is byte-unchanged.

This demo drives it over RPC and verifies, all without OCR:

  (A) Enter at the end of an indented line copies the indent and lands the
      caret past it.
  (B) A flush-left line yields a plain newline (no phantom indent).
  (C) A caret parked INSIDE the indent copies only the indent before it
      (never doubling the indentation).
  (D) The indent is taken from the CARET's line, not the first line.
  (E) Tabs count as indent characters and are copied verbatim.
  (F) Enter over a selection replaces it and copies the selection-start line's
      indent.
  (G) An auto-indented newline is ONE undo step (Ctrl+Z removes the newline and
      the copied indent together; Ctrl+Shift+Z re-applies both).
  (H) Shift+Enter is the same indent-aware newline (the modifier is ignored).
  (I) Repeated Enter inside a block keeps the block indentation on every line.

Text glyph rasterisation is covered by r904_syntax_highlight.py; this round
adds no new paint, so there is no separate live-pixel guard.

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r1268_auto_indent.py
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


def _caret(ed: RpcSubprocess, pos: int) -> None:
    ed.intervene(f"{_EXT}/selection", None)  # collapse any selection
    ed.intervene(f"{_EXT}/caret", pos)
    wait_until(lambda: ed.query(f"{_EXT}/caret") == pos, desc=f"caret parked at {pos}")


def _select(ed: RpcSubprocess, start: int, end: int) -> None:
    ed.intervene(f"{_EXT}/selection", {"start": start, "end": end})
    wait_until(
        lambda: ed.query(f"{_EXT}/selection") == {"start": start, "end": end},
        desc=f"selection == ({start}, {end})",
    )


def _text(ed: RpcSubprocess) -> str:
    return ed.query(f"{_EXT}/text")


def _cur(ed: RpcSubprocess) -> int:
    return ed.query(f"{_EXT}/caret")


def _key(ed: RpcSubprocess, key: str, *, shift: bool = False, ctrl: bool = False) -> Any:
    return ed.invoke(f"{_EXT}/key", {"key": key, "shift": shift, "ctrl": ctrl})


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── (A) Enter at the end of an indented line copies the indent ───────
        _set_text(ed, "    foo")
        _caret(ed, 7)  # end of the 4-space-indented line
        assert_eq(_key(ed, "Enter"), True, "Enter is handled (editor opted in)")
        wait_until(lambda: _text(ed) == "    foo\n    ", desc="new line copies the 4-space indent")
        assert_eq(_cur(ed), 12, "caret lands past the copied indent")

        # ── (B) a flush-left line yields a plain newline ─────────────────────
        _set_text(ed, "bar")
        _caret(ed, 3)
        assert_eq(_key(ed, "Enter"), True, "Enter handled on a flush-left line")
        wait_until(lambda: _text(ed) == "bar\n", desc="no phantom indent on a flush-left line")
        assert_eq(_cur(ed), 4, "caret after the bare newline")

        # ── (C) a caret INSIDE the indent copies only what precedes it ───────
        _set_text(ed, "    foo")
        _caret(ed, 2)  # parked between spaces 2 and 3 of the 4-space indent
        assert_eq(_key(ed, "Enter"), True, "Enter handled mid-indent")
        # Split at 2: "  " + "\n  " + "  foo" — the total indentation is preserved,
        # never doubled.
        wait_until(lambda: _text(ed) == "  \n    foo", desc="only the pre-caret indent is copied")
        assert_eq(_cur(ed), 5, "caret just past the 2-space copied indent")

        # ── (D) the indent comes from the CARET's line, not the first ────────
        _set_text(ed, "fn f() {\n        bar")
        _caret(ed, len("fn f() {\n        bar"))  # end of the 8-space 2nd line
        assert_eq(_key(ed, "Enter"), True, "Enter handled on the second line")
        wait_until(
            lambda: _text(ed) == "fn f() {\n        bar\n        ",
            desc="the newline copies the caret line's 8-space indent",
        )

        # ── (E) tabs are indent characters and are copied verbatim ───────────
        _set_text(ed, "\t\tx")
        _caret(ed, 3)  # after the two tabs + 'x'
        assert_eq(_key(ed, "Enter"), True, "Enter handled on a tab-indented line")
        wait_until(lambda: _text(ed) == "\t\tx\n\t\t", desc="the two leading tabs are copied")
        assert_eq(_cur(ed), 6, "caret past the two copied tabs")

        # ── (F) Enter over a selection copies the selection-start line indent ─
        _set_text(ed, "    ab\ncd")
        _select(ed, 4, 9)  # drains "ab\ncd"; insertion lands at byte 4 (line-1 start)
        assert_eq(_key(ed, "Enter"), True, "Enter handled while replacing a selection")
        wait_until(lambda: _text(ed) == "    \n    ", desc="indent copied from the selection-start line")
        assert_eq(_cur(ed), 9, "caret past the copied indent on the new line")

        # ── (G) an auto-indented newline is ONE undo step ────────────────────
        _set_text(ed, "    x")
        _caret(ed, 5)
        assert_eq(_key(ed, "Enter"), True, "Enter handled before undo")
        wait_until(lambda: _text(ed) == "    x\n    ", desc="auto-indented newline in place")
        assert_eq(_key(ed, "z", ctrl=True), True, "Ctrl+Z handled")
        wait_until(lambda: _text(ed) == "    x", desc="one undo removes the newline AND the indent")
        assert_eq(_key(ed, "z", ctrl=True, shift=True), True, "Ctrl+Shift+Z handled")
        wait_until(lambda: _text(ed) == "    x\n    ", desc="one redo re-applies both")

        # ── (H) Shift+Enter is the same indent-aware newline (modifier ignored) ─
        _set_text(ed, "    y")
        _caret(ed, 5)
        assert_eq(_key(ed, "Enter", shift=True), True, "Shift+Enter is handled")
        wait_until(lambda: _text(ed) == "    y\n    ", desc="Shift+Enter also auto-indents")

        # ── (I) repeated Enter keeps the block indentation on every line ─────
        _set_text(ed, "        deep")  # 8-space indent
        _caret(ed, len("        deep"))
        assert_eq(_key(ed, "Enter"), True, "first Enter handled")
        wait_until(lambda: _text(ed) == "        deep\n        ", desc="first new line indented")
        assert_eq(_key(ed, "Enter"), True, "second Enter handled")
        wait_until(
            lambda: _text(ed) == "        deep\n        \n        ",
            desc="the indent persists across successive newlines",
        )
        assert_eq(_cur(ed), len("        deep\n        \n        "), "caret at the deep indent")


if __name__ == "__main__":
    sys.exit(run_demo("R1268 auto-indent on Enter", body))
