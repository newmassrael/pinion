#!/usr/bin/env python3
"""R955 §5.22 §5.36 — keyboard-interactive code folding (fold navigator).

R933 made `hello-code-fold` a read-only fold VIEWER driven only over RPC. R955
wires the keyboard: while the viewer owns focus, `ArrowUp` / `ArrowDown` move a
current-line cursor over the *visible* logical lines (stepping over a collapsed
block, not into its hidden interior), `Home` / `End` jump to the first / last
visible line, and `Enter` / `Space` fold / unfold the region at the cursor. The
current line is highlighted. The keyboard drives the same reactive `TextEditState`
(`caret` / `toggle-fold`) an AI client drives over RPC — the two paths converge.

This demo drives it the AI-first way: it focuses the viewer, presses keys over the
§5.12 `scene/key` plane, and reads the cursor line (`caret`) + the fold set
(`fold_regions`) + the *painted* visible rows back — every read gated through
`wait_until` (observed-state polling, ZERO-FLAKE; folding is a pure transform of
text + collapsed-set, so the asserts are exact, never timing):

  (A) Arrow / Home / End move the cursor over visible logical lines.
  (B) Enter folds the region at the cursor; the read + painted gutter drop the
      interior rows; Enter again unfolds.
  (C) the inner block folds independently from the cursor on its opener.
  (D) ArrowDown steps OVER a collapsed block (never lands on a hidden line).
  (E) keyboard and RPC converge — an RPC `toggle-fold` unfolds what the keyboard
      folded.

Run from the workspace root:
    cargo build -p hello-code-fold --release
    python3 tools/demos/r955_code_fold_keyboard.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Iterator, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
    wait_until,
)

_EXT = "/external"
_EDITOR_TAG = "code_editor"

# The example's seed (set explicitly for determinism). Logical lines:
#   0: fn main() {        <- outer opener (closes line 5)
#   1:     let x = 1;
#   2:     if x > 0 {      <- inner opener (closes line 4)
#   3:         log(x);
#   4:     }               <- inner closer
#   5: }                   <- outer closer
_SEED = "fn main() {\n    let x = 1;\n    if x > 0 {\n        log(x);\n    }\n}"
_ALL_ROWS = {0, 1, 2, 3, 4, 5}


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{_EXT}/text", text)
    wait_until(lambda: ed.query(f"{_EXT}/text") == text, desc=f"buffer becomes {text!r}")


def _regions(ed: RpcSubprocess) -> list[dict[str, Any]]:
    r = ed.query(f"{_EXT}/fold_regions")
    assert isinstance(r, list), f"fold_regions must be a JSON array, got {r!r}"
    return r


def _collapsed_lines(ed: RpcSubprocess) -> set[int]:
    return {r["start_line"] for r in _regions(ed) if r["collapsed"]}


def _wait_collapsed(ed: RpcSubprocess, expected: set[int]) -> None:
    wait_until(lambda: _collapsed_lines(ed) == expected, desc=f"collapsed openers == {sorted(expected)}")


def _walk(node: Any) -> Iterator[dict[str, Any]]:
    stack = [node]
    while stack:
        n = stack.pop()
        if not isinstance(n, dict):
            continue
        yield n
        stack.extend(n.get("children") or [])
        content = n.get("content")
        if isinstance(content, dict):
            stack.append(content)


def _visible_rows(ed: RpcSubprocess) -> set[int]:
    """The logical-line indices whose row container is in the paint scene."""
    snap = ed.snapshot(source="paint")
    rows: set[int] = set()
    for n in _walk(snap):
        tag = n.get("tag")
        if isinstance(tag, str) and tag.startswith("fold_row_"):
            rows.add(int(tag[len("fold_row_") :]))
    return rows


def _wait_visible(ed: RpcSubprocess, expected: set[int]) -> None:
    wait_until(lambda: _visible_rows(ed) == expected, desc=f"visible rows == {sorted(expected)}")


def _caret_line(ed: RpcSubprocess) -> int:
    """The logical line the caret byte offset sits on (count of '\\n's before)."""
    c = ed.query(f"{_EXT}/caret")
    assert isinstance(c, int), f"caret is a byte offset int, got {c!r}"
    return _SEED.encode()[:c].count(b"\n")


def _wait_caret_line(ed: RpcSubprocess, line: int) -> None:
    wait_until(lambda: _caret_line(ed) == line, desc=f"cursor on logical line {line}")


def _key(ed: RpcSubprocess, name: str) -> None:
    ed.key(path=_EDITOR_TAG, name=name)


def body() -> None:
    with RpcSubprocess("hello-code-fold") as ed:
        _set_text(ed, _SEED)
        assert (
            ed.request("focus/set", {"tag": _EDITOR_TAG}).result.get("focused") == _EDITOR_TAG
        ), "the viewer takes shell focus so keys route to apply_key"
        ed.intervene(f"{_EXT}/caret", 0)
        _wait_caret_line(ed, 0)
        _wait_visible(ed, _ALL_ROWS)
        assert _collapsed_lines(ed) == set(), "nothing folded at the start"

        # -- (A) Arrow / Home / End move the cursor over visible lines -----
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 1)
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 2)
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 3)
        _key(ed, "ArrowUp")
        _wait_caret_line(ed, 2)
        _key(ed, "End")
        _wait_caret_line(ed, 5)
        _key(ed, "Home")
        _wait_caret_line(ed, 0)

        # -- (B) Enter folds the region at the cursor; gutter drops rows ----
        _key(ed, "Enter")  # cursor on line 0 = outer opener
        _wait_collapsed(ed, {0})
        _wait_visible(ed, {0})  # interior lines 1..5 hidden
        assert _caret_line(ed) == 0, "the cursor stays on the opener after folding"
        _key(ed, "Enter")  # unfold
        _wait_collapsed(ed, set())
        _wait_visible(ed, _ALL_ROWS)

        # -- (C) the inner block folds independently --------------------------
        _key(ed, "ArrowDown")
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 2)  # inner opener
        _key(ed, "Space")  # Space also toggles
        _wait_collapsed(ed, {2})
        _wait_visible(ed, {0, 1, 2, 5})  # interior 3,4 hidden, outer untouched
        assert 0 not in _collapsed_lines(ed), "the outer block stayed open"

        # -- (D) ArrowDown steps OVER a collapsed block -----------------------
        _wait_caret_line(ed, 2)
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 5)  # skipped hidden 3,4 to the next visible line
        assert _caret_line(ed) not in {3, 4}, "the cursor never lands on a hidden line"

        # -- (E) keyboard + RPC converge on one model -------------------------
        # Re-fold the inner from the keyboard, then unfold it over RPC.
        _key(ed, "Home")
        _key(ed, "ArrowDown")
        _key(ed, "ArrowDown")
        _wait_caret_line(ed, 2)
        assert _collapsed_lines(ed) == {2}, "inner still folded (RPC + keyboard share state)"
        assert ed.invoke(f"{_EXT}/toggle-fold", 2) is True, "RPC unfolds what the keyboard folded"
        _wait_collapsed(ed, set())
        _wait_visible(ed, _ALL_ROWS)


if __name__ == "__main__":
    sys.exit(run_demo("R955 §5.22 — keyboard-interactive code folding", body))
