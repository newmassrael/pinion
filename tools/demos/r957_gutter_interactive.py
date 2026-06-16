#!/usr/bin/env python3
"""R957 / R959 §5.22 §5.36 — interactive line-number gutter for the textarea.

Drives hello-textarea over JSON-RPC. R956 added the line-number gutter (one
number per logical line, aligned + scroll-synced). R957 made it interactive;
R959 re-routed the clicks to fix two defects:

  (A) **click a gutter number -> go to that line**: each number is tagged under
      the field's own composite namespace `main_textarea#gl<n>` (1-based logical
      line), so a click routes through the InputRouter to the field's `send`
      wire (`gl<n>:PointerUp`) -> `TextEditState::go_to_line(n)`. No pixel->line
      geometry; the line number IS the address;
  (B) **Shift+click -> extend the selection to that line**: the modifier rides
      the send wire (`gl<n>:PointerUp:s`); the field extends via
      `TextEditState::line_start_byte` (the byte-positioning peer of
      `go_to_line`, which would collapse the selection) — select whole lines;
  (C) **current-line highlight**: the gutter row the caret sits on carries a
      tagged band (`ta_gutter_current`) that tracks the caret line, exactly one
      per frame, aligned with that line's number.

R959 send-route fix (the defects the R957 first cut had — it decoded the gutter
inside the `position_caret_for_point` geometry press hook):

  (B2) **focus-independent**: a gutter click now FOCUSES the editor and moves
       the caret in one arc (click-to-focus resolves the `main_textarea#gl<n>`
       composite to the primary focusable field). The R957 cut short-circuited
       unless the field already held focus — a no-op gutter click;
  (B1) **no caret text-drag arm**: the composite hit-tag is rejected by the
       press hook (`!= main_textarea`), so a gutter press + jitter arms NO
       character text-selection. The R957 cut armed a text-drag on the smallest
       pointer movement.

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
FIELD = "main_textarea"
CURRENT = "ta_gutter_current"

# "alpha\nbeta\ngamma\ndelta\nepsilon" — five logical lines.
DOC = "alpha\nbeta\ngamma\ndelta\nepsilon"
LINE_START = [0, 6, 11, 17, 23]


def gnum(n: int) -> str:
    """R959 — the composite paint tag of the gutter's 1-based line number `n`,
    in the field's own send namespace (`gutter_line_sub_tag(FIELD, n)`)."""
    return f"{FIELD}#gl{n}"


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{EXT}/text", text)
    wait_query(ed, f"{EXT}/text", text, desc="buffer set")


def _gutter_rects(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    return {t: r for t, r in abs_rects_of(snap).items() if t.startswith(FIELD + "#gl")}


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

        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(5)) is not None,
            viewport=WIN,
            desc="gutter shows five numbers",
        )
        assert_eq(len(_gutter_rects(snap)), 5, "five gutter numbers")
        assert find_by_tag(snap, gnum(6)) is None, "no sixth number"

        # ── (B2) R959: a gutter click is FOCUS-INDEPENDENT. The field has not
        #         been clicked, so it is Idle; clicking a gutter number both
        #         moves the caret AND focuses the editor (click-to-focus
        #         resolves `main_textarea#gl<n>` to the primary field). The
        #         R957 press-hook cut no-op'd unless the field already held
        #         focus.
        wait_query(ed, f"{EXT}/state", "Idle", desc="editor is unfocused before any click")
        ed.click(path=gnum(3))
        wait_query(
            ed, f"{EXT}/caret", LINE_START[2],
            desc="gutter click moves the caret with no prior focus (B2)",
        )
        wait_query(ed, f"{EXT}/state", "Focused", desc="the gutter click focused the editor (B2)")

        # ── (A) click each gutter number -> caret jumps to that line ─────
        for n in range(1, 6):
            ed.click(path=gnum(n))
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
                and abs_rects_of(s).get(CURRENT) == abs_rects_of(s).get(gnum(ln)),
                viewport=WIN,
                desc=f"current band aligns with gutter number {n}",
            )
            assert_eq(_band_count(snap), 1, f"exactly one current-line band at line {n}")
            rects = abs_rects_of(snap)
            assert_eq(
                rects[CURRENT],
                rects[gnum(n)],
                f"band rect == gutter number {n} rect",
            )

        # ── (B) clicking is idempotent + the jump collapses any selection ─
        ed.click(path=gnum(3))
        wait_query(ed, f"{EXT}/caret", LINE_START[2], desc="caret at line 3")
        assert_eq(ed.query(f"{EXT}/selection"), None, "a plain gutter click collapses selection")
        ed.click(path=gnum(3))
        assert_eq(ed.query(f"{EXT}/caret"), LINE_START[2], "re-clicking line 3 is idempotent")

        # ── (B) Shift+click extends the selection to that line ──────────
        ed.click(path=gnum(2))  # caret -> line 2 start (byte 6), anchor here
        wait_query(ed, f"{EXT}/caret", LINE_START[1], desc="caret at line 2 (anchor)")
        ed.modifiers(shift=True)
        ed.click(path=gnum(5))  # extend to line 5 start (byte 23)
        ed.modifiers()  # release Shift
        wait_query(
            ed,
            f"{EXT}/selection",
            {"start": LINE_START[1], "end": LINE_START[4]},
            desc="Shift+click extends selection from line 2 to line 5 start",
        )

        # Shift+click UPWARD: collapse first, then extend up to an earlier line.
        ed.click(path=gnum(4))  # caret -> line 4 (byte 17), selection collapses
        wait_query(ed, f"{EXT}/caret", LINE_START[3], desc="caret at line 4")
        assert_eq(ed.query(f"{EXT}/selection"), None, "the plain click collapsed the prior selection")
        ed.modifiers(shift=True)
        ed.click(path=gnum(1))  # extend up to line 1 start (byte 0)
        ed.modifiers()
        wait_query(
            ed,
            f"{EXT}/selection",
            {"start": LINE_START[0], "end": LINE_START[3]},
            desc="Shift+click upward extends from line 4 to line 1 start",
        )

        # ── (B1) R959: a drag that STARTS on a gutter number arms NO character
        #         text-selection (the R957 press-hook cut armed a text-drag on
        #         the smallest pointer jitter). Park the caret with a plain
        #         click (selection collapses), then drag off a gutter number —
        #         the selection stays empty.
        ed.click(path=gnum(2))
        wait_query(ed, f"{EXT}/caret", LINE_START[1], desc="caret parked at line 2")
        assert_eq(ed.query(f"{EXT}/selection"), None, "caret parked, no selection")
        ed.drag(from_path=gnum(3), to_path=gnum(5), steps=6)
        assert_eq(
            ed.query(f"{EXT}/selection"),
            None,
            "a drag starting on a gutter number arms no text-selection (B1)",
        )

        # ── (C) the band follows a keyboard caret move too (not just clicks)
        ed.invoke(f"{EXT}/go-to-line", 5)
        wait_query(ed, f"{EXT}/caret", LINE_START[4], desc="go-to-line 5 (RPC) moves the caret")
        snap = wait_snap(
            ed,
            lambda s: abs_rects_of(s).get(CURRENT) == abs_rects_of(s).get(gnum(5)),
            viewport=WIN,
            desc="current band follows the RPC caret move to line 5",
        )
        assert_eq(_band_count(snap), 1, "still exactly one current-line band")
        assert_eq(
            abs_rects_of(snap)[CURRENT],
            abs_rects_of(snap)[gnum(5)],
            "band aligned with line 5 after the keyboard-path move",
        )

        # ── (D) gutter geometry sanity: numbers left of the field, in order
        snap = ed.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)
        field = rects[FIELD]
        for n in range(1, 6):
            gx = rects[gnum(n)]
            assert gx[0] + gx[2] <= field[0], f"gutter number {n} is left of the field"
        ys = [rects[gnum(n)][1] for n in range(1, 6)]
        assert all(ys[i] < ys[i + 1] for i in range(4)), f"gutter numbers in increasing y: {ys}"


if __name__ == "__main__":
    sys.exit(run_demo("R957 §5.22 §5.36 — interactive line-number gutter", body))
