#!/usr/bin/env python3
"""R957 §5.22 §5.36 — interactive line-number gutter for the textarea.

Drives hello-textarea over JSON-RPC. R956 added the line-number gutter (one
number per logical line, aligned + scroll-synced). R957 makes it interactive:

  (A) **click a gutter number -> go to that line**: the number's composite tag
      `ta_gutter#<n>` carries the 1-based logical line, so a click resolves
      straight to `TextEditState::go_to_line(n)` (no pixel->line geometry) —
      the caret jumps to the line's start;
  (B) **Shift+click -> extend the selection to that line**: via the new pure
      `TextEditState::line_start_byte` (the byte-positioning peer of
      `go_to_line`, which would collapse the selection) — select whole lines;
  (C) **current-line highlight**: the gutter row the caret sits on carries a
      tagged band (`ta_gutter_current`) that tracks the caret line, exactly one
      per frame, aligned with that line's number.

The gutter clicks flow through the binding's `position_caret_for_point` press
hook (the field stays focused — the gutter is a non-focusable decoration, so a
click on it leaves focus on the editor per the W3C convention), so this is a
real pointer arc, not an External invoke.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r957_gutter_interactive.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (480, 320)
EXT = "/external"
GUT = "ta_gutter"
CURRENT = "ta_gutter_current"

# "alpha\nbeta\ngamma\ndelta\nepsilon" — five logical lines.
DOC = "alpha\nbeta\ngamma\ndelta\nepsilon"
LINE_START = [0, 6, 11, 17, 23]


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{EXT}/text", text)
    wait_query(ed, f"{EXT}/text", text, desc="buffer set")


def _gutter_rects(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    return {t: r for t, r in abs_rects_of(snap).items() if t.startswith(GUT + "#")}


def _band_count(snap: Any) -> int:
    """Count current-line bands in the paint tree (must be exactly one)."""
    n = 0

    def walk(node: Any) -> None:
        nonlocal n
        if not isinstance(node, dict):
            return
        if node.get("tag") == CURRENT:
            n += 1
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(snap)
    return n


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ed:
        _set_text(ed, DOC)
        assert_eq(ed.query(f"{EXT}/line_count"), 5, "five logical lines")
        # Focus the editor so gutter clicks reach the caret hook (the gutter
        # is a non-focusable decoration; the editor must already hold focus).
        ed.click(path="main_textarea")

        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, f"{GUT}#5") is not None,
            viewport=WIN,
            desc="gutter shows five numbers",
        )
        assert_eq(len(_gutter_rects(snap)), 5, "five gutter numbers")
        assert find_by_tag(snap, f"{GUT}#6") is None, "no sixth number"

        # ── (A) click each gutter number -> caret jumps to that line ─────
        for n in range(1, 6):
            ed.click(path=f"{GUT}#{n}")
            wait_query(
                ed,
                f"{EXT}/caret",
                LINE_START[n - 1],
                desc=f"click gutter {n} -> caret at line {n} start (byte {LINE_START[n-1]})",
            )
            # (C) the current-line band tracks the clicked line: exactly one
            # band, aligned with that line's number.
            snap = wait_snap(
                ed,
                lambda s, ln=n: find_by_tag(s, CURRENT) is not None
                and abs_rects_of(s).get(CURRENT) == abs_rects_of(s).get(f"{GUT}#{ln}"),
                viewport=WIN,
                desc=f"current band aligns with gutter number {n}",
            )
            assert_eq(_band_count(snap), 1, f"exactly one current-line band at line {n}")
            rects = abs_rects_of(snap)
            assert_eq(
                rects[CURRENT],
                rects[f"{GUT}#{n}"],
                f"band rect == gutter number {n} rect",
            )

        # ── (B) clicking is idempotent + the jump collapses any selection ─
        ed.click(path=f"{GUT}#3")
        wait_query(ed, f"{EXT}/caret", LINE_START[2], desc="caret at line 3")
        assert_eq(ed.query(f"{EXT}/selection"), None, "a plain gutter click collapses selection")
        ed.click(path=f"{GUT}#3")
        assert_eq(ed.query(f"{EXT}/caret"), LINE_START[2], "re-clicking line 3 is idempotent")

        # ── (B) Shift+click extends the selection to that line ──────────
        ed.click(path=f"{GUT}#2")  # caret -> line 2 start (byte 6), anchor here
        wait_query(ed, f"{EXT}/caret", LINE_START[1], desc="caret at line 2 (anchor)")
        ed.modifiers(shift=True)
        ed.click(path=f"{GUT}#5")  # extend to line 5 start (byte 23)
        ed.modifiers()  # release Shift
        wait_query(
            ed,
            f"{EXT}/selection",
            {"start": LINE_START[1], "end": LINE_START[4]},
            desc="Shift+click extends selection from line 2 to line 5 start",
        )

        # Shift+click UPWARD: collapse first, then extend up to an earlier line.
        ed.click(path=f"{GUT}#4")  # caret -> line 4 (byte 17), selection collapses
        wait_query(ed, f"{EXT}/caret", LINE_START[3], desc="caret at line 4")
        assert_eq(ed.query(f"{EXT}/selection"), None, "the plain click collapsed the prior selection")
        ed.modifiers(shift=True)
        ed.click(path=f"{GUT}#1")  # extend up to line 1 start (byte 0)
        ed.modifiers()
        wait_query(
            ed,
            f"{EXT}/selection",
            {"start": LINE_START[0], "end": LINE_START[3]},
            desc="Shift+click upward extends from line 4 to line 1 start",
        )

        # ── (C) the band follows a keyboard caret move too (not just clicks)
        ed.invoke(f"{EXT}/go-to-line", 5)
        wait_query(ed, f"{EXT}/caret", LINE_START[4], desc="go-to-line 5 (RPC) moves the caret")
        snap = wait_snap(
            ed,
            lambda s: abs_rects_of(s).get(CURRENT) == abs_rects_of(s).get(f"{GUT}#5"),
            viewport=WIN,
            desc="current band follows the RPC caret move to line 5",
        )
        assert_eq(_band_count(snap), 1, "still exactly one current-line band")
        assert_eq(
            abs_rects_of(snap)[CURRENT],
            abs_rects_of(snap)[f"{GUT}#5"],
            "band aligned with line 5 after the keyboard-path move",
        )

        # ── (D) gutter geometry sanity: numbers left of the field, in order
        snap = ed.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)
        field = rects["main_textarea"]
        for n in range(1, 6):
            gx = rects[f"{GUT}#{n}"]
            assert gx[0] + gx[2] <= field[0], f"gutter number {n} is left of the field"
        ys = [rects[f"{GUT}#{n}"][1] for n in range(1, 6)]
        assert all(ys[i] < ys[i + 1] for i in range(4)), f"gutter numbers in increasing y: {ys}"


if __name__ == "__main__":
    sys.exit(run_demo("R957 §5.22 §5.36 — interactive line-number gutter", body))
