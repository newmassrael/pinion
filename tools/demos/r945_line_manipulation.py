#!/usr/bin/env python3
"""R945 §5.22 — move-line / duplicate-line on a multi-line code editor.

Drives hello-syntax-highlight over JSON-RPC. The editor could already indent
(R938), comment (R939) and jump to a line (R941); R945 adds the two universal
line-manipulation commands every code editor has — MOVE a line (VS Code
`Alt+Up` / `Alt+Down`) and DUPLICATE a line (`Shift+Alt+Up` / `Shift+Alt+Down`)
— as `TextEditState` substrate (reusable by every multi-line field) plus the
AI-first invoke verbs:

  * `move-line-up` / `move-line-down` swap the current line (or selected line
    block) with the adjacent line, returning `Bool` — whether the buffer
    changed (a boundary move — first line up, last line down — is `false`);
  * `duplicate-line-up` / `duplicate-line-down` insert a copy of the line
    below, the `down` flag choosing whether the caret lands on the lower copy
    or stays on the upper instance; always `Bool(true)`.

  (A) move-line round-trips down then up (the buffer returns to its order).
  (B) a boundary move is a Bool(false) no-op.
  (C) moving across the final, newline-less line keeps the newline structure.
  (D) duplicate-line down / up insert a copy; the caret lands per `down`.
  (E) duplicating the last (newline-less) line adds a separator newline.
  (F) move / duplicate are each ONE undo step (Ctrl+Z reverses the whole op).
  (G) the `caret` query agrees with the edit (AI-first observability).

  The Alt+Up / Alt+Down GUI chords are the documented follow-up (Alt+Arrow
  overlaps vertical caret nav, so the chord placement waits on that keymap
  arm); this round lands the §2-primary RPC path + substrate, the R929 / R931 /
  R941 RPC-primary precedent.

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r945_line_manipulation.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

# hello-syntax-highlight's primary External answers at /external.
_EXT = "/external"


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{_EXT}/text", text)
    wait_until(lambda: ed.query(f"{_EXT}/text") == text, desc="buffer set")


def _text(ed: RpcSubprocess) -> str:
    return ed.query(f"{_EXT}/text")


def _caret(ed: RpcSubprocess) -> int:
    return ed.query(f"{_EXT}/caret")


def _set_caret(ed: RpcSubprocess, pos: int) -> None:
    # Collapse the selection onto `pos` (an empty range), then confirm.
    ed.intervene(f"{_EXT}/selection", {"start": pos, "end": pos})
    wait_until(lambda: _caret(ed) == pos, desc=f"caret at {pos}")


def _select(ed: RpcSubprocess, start: int, end: int) -> None:
    ed.intervene(f"{_EXT}/selection", {"start": start, "end": end})
    wait_until(
        lambda: ed.query(f"{_EXT}/selection") == {"start": start, "end": end},
        desc=f"selection == ({start}, {end})",
    )


def _move(ed: RpcSubprocess, down: bool):
    return ed.invoke(f"{_EXT}/move-line-{'down' if down else 'up'}", None)


def _dup(ed: RpcSubprocess, down: bool):
    return ed.invoke(f"{_EXT}/duplicate-line-{'down' if down else 'up'}", None)


def _undo(ed: RpcSubprocess) -> None:
    ed.invoke(f"{_EXT}/key", {"key": "z", "ctrl": True})


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── (A) move-line down then up round-trips ──────────────────────────
        _set_text(ed, "a\nb\nc")  # starts [0, 2, 4]
        _set_caret(ed, 0)  # caret on line "a"
        assert_eq(_move(ed, True), True, "move-line-down reports a change")
        wait_until(lambda: _text(ed) == "b\na\nc", desc="line a swaps below b")
        assert_eq(_text(ed), "b\na\nc", "a moved past b")
        assert_eq(_caret(ed), 2, "caret rides the moved line to its new start")
        assert_eq(_move(ed, False), True, "move-line-up reports a change")
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="up restores the order")
        assert_eq(_text(ed), "a\nb\nc", "the order is restored")
        assert_eq(_caret(ed), 0, "caret rides back up")

        # ── (B) boundary moves are Bool(false) no-ops ───────────────────────
        _set_caret(ed, 0)
        assert_eq(_move(ed, False), False, "the first line cannot move up")
        assert_eq(_text(ed), "a\nb\nc", "no change from the boundary move")
        _set_caret(ed, 4)  # last line "c"
        assert_eq(_move(ed, True), False, "the last line cannot move down")
        assert_eq(_text(ed), "a\nb\nc", "still no change")

        # ── (C) moving across the final, newline-less line ──────────────────
        # "c" has no trailing newline; moving "b" down past it must keep exactly
        # one line break between them and no trailing newline.
        _set_caret(ed, 2)  # line "b"
        assert_eq(_move(ed, True), True, "b moves down past the last line")
        wait_until(lambda: _text(ed) == "a\nc\nb", desc="b is now the last line")
        assert_eq(_text(ed), "a\nc\nb", "the newline relocated; no trailing newline added")
        # And back up: "b" returns to the middle, "c" back to the end.
        assert_eq(_move(ed, False), True, "b moves back up")
        assert_eq(_text(ed), "a\nb\nc", "the pair swap restores the buffer exactly")

        # ── (D) duplicate-line down / up ────────────────────────────────────
        _set_caret(ed, 2)  # line "b"
        assert_eq(_dup(ed, True), True, "duplicate-line-down always inserts")
        wait_until(lambda: _text(ed) == "a\nb\nb\nc", desc="a copy of b appears below")
        assert_eq(_text(ed), "a\nb\nb\nc", "the line is duplicated")
        assert_eq(_caret(ed), 4, "down lands the caret on the lower copy")
        _undo(ed)
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="undo removes the copy")

        _set_caret(ed, 2)
        assert_eq(_dup(ed, False), True, "duplicate-line-up inserts a copy too")
        assert_eq(_text(ed), "a\nb\nb\nc", "the buffer gains an identical adjacent copy")
        assert_eq(_caret(ed), 2, "up keeps the caret on the upper instance")
        _undo(ed)
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="undo removes the copy")

        # ── (E) duplicating the final, newline-less line adds a separator ───
        _set_caret(ed, 4)  # line "c" (no trailing newline)
        assert_eq(_dup(ed, True), True, "duplicate the last line")
        wait_until(lambda: _text(ed) == "a\nb\nc\nc", desc="a separator newline precedes the copy")
        assert_eq(_text(ed), "a\nb\nc\nc", "the copy is newline-separated")
        assert_eq(_caret(ed), 6, "the caret lands on the lower copy")
        _undo(ed)
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="undo restores the buffer")

        # ── (F) move-line is ONE undo step ──────────────────────────────────
        _set_caret(ed, 0)
        _move(ed, True)
        wait_until(lambda: _text(ed) == "b\na\nc", desc="moved before undo")
        _undo(ed)
        wait_until(lambda: _text(ed) == "a\nb\nc", desc="one undo reverses the whole move")
        assert_eq(_text(ed), "a\nb\nc", "the move is a single undo step")

        # ── (G) a multi-line selection moves as a block; the caret agrees ───
        _set_text(ed, "a\nb\nc\nd")  # starts [0, 2, 4, 6]
        _select(ed, 0, 3)  # lines "a" and "b"
        assert_eq(_move(ed, True), True, "the two-line block moves down past c")
        wait_until(lambda: _text(ed) == "c\na\nb\nd", desc="the block swaps past c")
        assert_eq(_text(ed), "c\na\nb\nd", "the whole block moved together")
        # The selection re-covers the moved block "a\nb\n" = bytes [2, 6).
        assert_eq(ed.query(f"{_EXT}/selection"), {"start": 2, "end": 6}, "the block re-covers")

        # ── (H) bare numeric check on a taller buffer (AI-first read) ───────
        _set_text(ed, "\n".join("L" + str(n) for n in range(6)))  # L0..L5, lines of 2 chars + \n
        _set_caret(ed, 0)  # L0
        assert_eq(_move(ed, True), True, "L0 moves down")
        assert_eq(_text(ed), "L1\nL0\nL2\nL3\nL4\nL5", "only L0 and L1 swapped")
        assert_eq(_caret(ed), 3, "caret on L0 at its new start (byte 3)")
        assert_eq(_dup(ed, True), True, "duplicate L0")
        assert_eq(_text(ed), "L1\nL0\nL0\nL2\nL3\nL4\nL5", "L0 now appears twice")


if __name__ == "__main__":
    sys.exit(run_demo("R945 §5.22 — move / duplicate line", body))
