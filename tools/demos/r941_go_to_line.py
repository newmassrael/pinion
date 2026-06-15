#!/usr/bin/env python3
"""R941 §5.22 — go-to-line navigation (TextEditState::go_to_line + RPC verb).

Drives hello-syntax-highlight over JSON-RPC. The editor could find/replace
(R903), syntax-highlight (R904), match brackets (R926), fold (R933), indent
(R938) and toggle comments (R939) — all line-AWARE, but it could not JUMP the
caret to a line by number (the universal Ctrl+G editor navigation). R941 adds
`TextEditState::go_to_line` (substrate, reusable by every multi-line field) +
the AI-first `go-to-line` RPC verb + a `line_count` query:

  * `go-to-line` (arg = a 1-based line Int) moves the caret to that line's
    start, collapsing any selection, and RETURNS the resolved line (clamped to
    `1..=line_count`) — the setter-returns-read-outcome contract, so an agent
    learns the actual destination in one round-trip;
  * `line_count` is the logical (newline-delimited) line count — the navigation
    bound (and a line-number gutter / prompt max).

  (A) `line_count` tracks the buffer.
  (B) `go-to-line` jumps the caret to each line's start + echoes the line.
  (C) out-of-range lines clamp (0 -> first, past-end -> last).
  (D) a jump collapses an active selection (a caret move).
  (E) the `caret` query agrees with the jump (AI-first observability).

  The Ctrl+G GUI prompt (a modal line-number input) is the documented GUI
  follow-up; this round lands the §2-primary RPC path + substrate (the
  R929/R931/R933 RPC-primary precedent).

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r941_go_to_line.py

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

# hello-syntax-highlight's TF_TAG; the single primary External answers at /external.
_EXT = "/external"

# A 5-line program; line starts (byte offsets) are [0, 12, 27, 42, 52].
DOC = "fn main() {\n    let x = 1;\n    let y = 2;\n    x + y\n}"
LINE_START = [0, 12, 27, 42, 52]


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{_EXT}/text", text)
    wait_until(lambda: ed.query(f"{_EXT}/text") == text, desc="buffer set")


def _select(ed: RpcSubprocess, start: int, end: int) -> None:
    ed.intervene(f"{_EXT}/selection", {"start": start, "end": end})
    wait_until(
        lambda: ed.query(f"{_EXT}/selection") == {"start": start, "end": end},
        desc=f"selection == ({start}, {end})",
    )


def _goto(ed: RpcSubprocess, line: int):
    return ed.invoke(f"{_EXT}/go-to-line", line)


def _caret(ed: RpcSubprocess) -> int:
    return ed.query(f"{_EXT}/caret")


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── (A) line_count tracks the buffer ────────────────────────
        _set_text(ed, DOC)
        assert_eq(ed.query(f"{_EXT}/line_count"), 5, "the 5-line program")
        _set_text(ed, "solo")
        assert_eq(ed.query(f"{_EXT}/line_count"), 1, "a single line, no newline")
        _set_text(ed, "a\nb\n")
        assert_eq(ed.query(f"{_EXT}/line_count"), 3, "a trailing newline opens an empty last line")

        # ── (B) go-to-line jumps the caret to each line's start ─────
        _set_text(ed, DOC)
        for line in range(1, 6):
            assert_eq(_goto(ed, line), line, f"go-to-line {line} echoes the resolved line")
            wait_until(
                lambda ln=line: _caret(ed) == LINE_START[ln - 1],
                desc=f"caret at line {line} start (byte {LINE_START[line - 1]})",
            )
            assert_eq(_caret(ed), LINE_START[line - 1], f"caret query agrees for line {line}")

        # ── (C) out-of-range lines clamp ────────────────────────────
        assert_eq(_goto(ed, 0), 1, "line 0 clamps up to the first line")
        wait_until(lambda: _caret(ed) == 0, desc="caret at the first line")
        assert_eq(_goto(ed, 999), 5, "a line past the end clamps to the last line")
        wait_until(lambda: _caret(ed) == LINE_START[4], desc="caret at the last line")
        assert_eq(_caret(ed), 52, "the clamped destination is the last line's start")

        # ── (D) a jump collapses an active selection ────────────────
        _select(ed, LINE_START[1], LINE_START[3])  # span lines 2-4
        assert _goto(ed, 1) == 1, "jump while a selection is active"
        wait_until(lambda: _caret(ed) == 0, desc="caret jumped to line 1")
        assert_eq(ed.query(f"{_EXT}/selection"), None, "the jump collapsed the selection")

        # ── (E) idempotent + AI-first observability ─────────────────
        assert_eq(_goto(ed, 3), 3, "jump to line 3")
        assert_eq(_caret(ed), 27, "caret at line 3 start")
        assert_eq(_goto(ed, 3), 3, "re-jumping to the same line is idempotent")
        assert_eq(_caret(ed), 27, "caret unchanged on the idempotent jump")
        # line_count is unchanged by navigation (a read-only bound).
        assert_eq(ed.query(f"{_EXT}/line_count"), 5, "navigation does not change the line count")

        # ── (F) explicit interior jumps + edge buffers ──────────────
        # Jump around the interior out of order (no reliance on the loop above).
        assert_eq(_goto(ed, 2), 2, "jump to line 2")
        assert_eq(_caret(ed), 12, "line 2 starts at byte 12")
        assert_eq(_goto(ed, 4), 4, "jump to line 4")
        assert_eq(_caret(ed), 42, "line 4 starts at byte 42")
        assert_eq(_goto(ed, 1), 1, "back to line 1")
        assert_eq(_caret(ed), 0, "line 1 starts at byte 0")
        # A single-line buffer: every line clamps to line 1 (byte 0).
        _set_text(ed, "single line, no newline")
        assert_eq(ed.query(f"{_EXT}/line_count"), 1, "one line")
        assert_eq(_goto(ed, 1), 1, "line 1 of a single-line buffer")
        assert_eq(_caret(ed), 0, "single-line caret at start")
        assert_eq(_goto(ed, 5), 1, "a 1-line buffer clamps any line to 1")
        assert_eq(_caret(ed), 0, "clamped to byte 0")
        # A taller buffer re-derives the count + a deep jump live.
        _set_text(ed, "\n".join(str(n) for n in range(20)))  # 20 lines "0".."19"
        assert_eq(ed.query(f"{_EXT}/line_count"), 20, "20 logical lines")
        assert_eq(_goto(ed, 20), 20, "jump to the last of 20 lines")
        # Lines 1-10 ("0".."9") are 1 char + \n = 2 bytes (line 11 at 20); lines
        # 11-20 ("10".."19") are 2 chars + \n = 3 bytes, so line 20 ("19") starts
        # at 20 + (20 - 11) * 3 = 47.
        assert_eq(_caret(ed), 47, "line 20 ('19') starts at byte 47")


if __name__ == "__main__":
    sys.exit(run_demo("R941 §5.22 — go-to-line navigation", body))
