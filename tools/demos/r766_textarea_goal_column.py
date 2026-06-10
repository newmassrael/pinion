#!/usr/bin/env python3
"""R766 §5.22 §5.36 — textarea goal column + visual-line Home/End.

R765 landed soft-wrap + scroll-into-view. Two textarea-correctness debts
remained from the R764/R765 vertical-navigation work; R766 clears both:

(d) **Goal column persistence.** Vertical caret motion (`ArrowUp` /
    `ArrowDown`) must hold the *original* column across a run, even when
    an intervening line is short. parley maintains an `h_pos` inside its
    own `Selection`, but pinion's caret is a geometry-free byte offset
    reshaped each frame, so each move was re-seeding the column from the
    (short-line-clamped) current caret and drifting. R766 persists the
    goal in `TextEditState` and overrides the drifted column via
    `Cursor::from_point` on the resolved line.

(e) **Visual-line Home / End.** `Home` / `End` move to the start / end
    of the current *visual* row (soft-wrap aware) — not the buffer ends
    the single-line field uses.

This is a pure DATA proof. The goal-column fixture uses a single repeated
character so a column maps to an exact byte offset on every full line:

    L0 = "w"*12   (bytes  0..12)   '\\n' @ 12
    L1 = "w"*2    (bytes 13..15)   '\\n' @ 15   <- the SHORT line
    L2 = "w"*12   (bytes 16..28)

Because every glyph has the same advance, column N on L0 and L2 sit at
the identical x, so the goal column restores to an exact byte:
ArrowDown from L0 col 5 (byte 5) clamps to L1 col 2 (byte 15), and the
next ArrowDown into L2 returns to col 5 (byte 21), NOT the short line's
col 2 (byte 18). A horizontal move in between resets the goal.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r766_textarea_goal_column.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo, wait_query, wait_until

TA_TAG = "main_textarea"
VIEWPORT = (480, 320)

# Goal-column fixture: long / short / long, all one repeated glyph so a
# column is an exact byte on the full lines.
L = "w" * 12
SHORT = "w" * 2
GOAL_TEXT = f"{L}\n{SHORT}\n{L}"  # bytes: L0 0..12, L1 13..15, L2 16..28
L0_START, L1_START, L2_START = 0, 13, 16

# A long no-newline line that wraps onto several visual rows (width-driven
# break, 0 newlines) — for the visual Home / End proof.
LONG = (
    "the quick brown fox jumps over the lazy dog while the "
    "sleepy cat watches the whole thing unfold slowly today"
)


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def selection(ta: RpcSubprocess):
    return ta.query("/external/selection")


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def key(ta: RpcSubprocess, name: str) -> None:
    ta.key(path=TA_TAG, name=name)


def key_shift(ta: RpcSubprocess, name: str) -> None:
    ta.modifiers(shift=True)
    key(ta, name)
    ta.modifiers()


def key_ctrl(ta: RpcSubprocess, name: str) -> None:
    ta.modifiers(ctrl=True)
    key(ta, name)
    ta.modifiers()


def select_all(ta: RpcSubprocess) -> None:
    ta.modifiers(ctrl=True)
    key(ta, "a")
    ta.modifiers()


def replace_all(ta: RpcSubprocess, body: str) -> None:
    select_all(ta)
    ta.text(body, path=TA_TAG)
    wait_until(
        lambda: text(ta) == body,
        desc="replace_all: typed body landed in the buffer",
    )


def set_multiline(ta: RpcSubprocess, lines: list[str]) -> None:
    """Type a multi-line buffer the way a user would: clear, then type
    each line with an `Enter` keypress between them. The text-insert RPC
    carries no newline (only the `Enter` key inserts one), so a hard
    multi-line fixture is built from keystrokes, not a single payload."""
    select_all(ta)
    key(ta, "Backspace")  # delete the selection -> empty buffer
    for i, ln in enumerate(lines):
        if i > 0:
            key(ta, "Enter")
        ta.text(ln, path=TA_TAG)


def goto_start(ta: RpcSubprocess) -> None:
    """Caret to byte 0. `Home` is now visual-line-relative (R766), so use
    `Ctrl+Home` (the document-start boundary R766 added)."""
    key_ctrl(ta, "Home")


def goto_column(ta: RpcSubprocess, col: int) -> None:
    """Caret to `col` of the first line: buffer start, then ArrowRight."""
    goto_start(ta)
    for _ in range(col):
        key(ta, "ArrowRight")


def field_rect(ta: RpcSubprocess):
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, TA_TAG)
    assert node is not None, "textarea present in paint scene"
    r = node["rect"]
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def caret_paint_x(ta: RpcSubprocess):
    """The painted caret's x in the *actual* paint scene (from:paint): the
    slim caret_width=2 `Box`. This is pixel-space evidence — the column
    the user sees — not just the byte offset. Returns None if unfocused."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    boxes = [
        n
        for n in _walk(field, [])
        if n.get("type") == "Box" and n.get("rect", {}).get("w") == 2
    ]
    return boxes[-1]["rect"]["x"] if boxes else None


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="focused after focus/set")

        # ════════════════════════════════════════════════════════════
        #  (d) GOAL COLUMN — persists across a short line, both ways
        # ════════════════════════════════════════════════════════════
        set_multiline(ta, [L, SHORT, L])
        assert_eq(text(ta), GOAL_TEXT, "goal fixture loaded")
        assert_eq(text(ta).count("\n"), 2, "goal fixture has 3 hard lines")

        # Seed caret at column 5 of line 0.
        goto_column(ta, 5)
        assert_eq(caret(ta), 5, "caret seeded at L0 column 5")
        # Pixel-space anchor: the painted caret's x at L0 col 5. Because
        # every glyph is the same width, the restored caret on L2 col 5
        # must land at this same painted x.
        x_col5 = caret_paint_x(ta)
        assert x_col5 is not None, "painted caret present at L0 col 5"

        # ArrowDown into the SHORT line clamps the column to its end.
        key(ta, "ArrowDown")
        assert_eq(caret(ta), L1_START + 2, "ArrowDown clamps to the short line's end (col 2)")
        x_short = caret_paint_x(ta)
        assert x_short < x_col5 - 1, (
            f"painted caret on the short line sits left of col 5 (x {x_short} < {x_col5})"
        )

        # ArrowDown again into the long line RESTORES column 5 (the goal
        # rode along) — byte 21, not the short line's drifted col 2 (18).
        key(ta, "ArrowDown")
        assert_eq(
            caret(ta),
            L2_START + 5,
            "goal column restores L2 col 5 (byte 21), not the drifted col 2 (18)",
        )
        x_restored = caret_paint_x(ta)
        assert abs(x_restored - x_col5) <= 1, (
            f"the PAINTED caret column is restored on L2 (x {x_restored} ~= {x_col5}) — "
            f"pixel-space proof the user sees the original column, not the drift"
        )

        # And it holds going UP through the short line too.
        key(ta, "ArrowUp")
        assert_eq(caret(ta), L1_START + 2, "ArrowUp clamps to the short line's end")
        key(ta, "ArrowUp")
        assert_eq(caret(ta), L0_START + 5, "goal column restores L0 col 5 going up (byte 5)")

        # ── goal RESETS on a horizontal move ─────────────────────────
        goto_column(ta, 5)
        key(ta, "ArrowDown")  # -> L1 col 2 (byte 15)
        assert_eq(caret(ta), L1_START + 2, "down to short line again")
        key(ta, "ArrowLeft")  # horizontal move -> byte 14 (col 1), resets goal
        assert_eq(caret(ta), L1_START + 1, "ArrowLeft moves to L1 col 1 and resets the goal")
        key(ta, "ArrowDown")  # goal re-seeded from col 1 -> L2 col 1 (byte 17)
        assert_eq(
            caret(ta),
            L2_START + 1,
            "after the horizontal move the goal follows col 1 (byte 17), not the old col 5",
        )

        # ── goal RESETS on a click ───────────────────────────────────
        goto_column(ta, 6)
        key(ta, "ArrowDown")  # short line clamps
        (fx, fy, fw, fh) = field_rect(ta)
        ta.click(at=(fx + 4, fy + 8 + 12))  # click line 0, near the left
        wait_until(
            lambda: caret(ta) <= L0_START + 3,
            desc="click landed on L0 near the start",
        )
        c_click = caret(ta)
        assert c_click <= L0_START + 3, f"click landed on L0 near the start (byte {c_click})"
        key(ta, "ArrowDown")  # goal re-seeded from the click column
        c_after = caret(ta)
        assert c_after < L2_START + 5, (
            f"after the click the goal follows the click column, not col 6 "
            f"(byte {c_after} < {L2_START + 5})"
        )

        # ── goal RESETS on an edit ───────────────────────────────────
        goto_column(ta, 7)
        key(ta, "ArrowDown")  # short line
        ta.text("z", path=TA_TAG)  # insert resets the goal
        wait_until(
            lambda: text(ta).count("z") == 1,
            desc="an edit inserted a glyph on the short line",
        )

        # ════════════════════════════════════════════════════════════
        #  (e) VISUAL-LINE Home / End on a hard-newline buffer
        # ════════════════════════════════════════════════════════════
        set_multiline(ta, [L, SHORT, L])
        # Put the caret inside line 2, then Home/End stay within line 2.
        goto_column(ta, 5)
        key(ta, "ArrowDown")
        key(ta, "ArrowDown")  # now on L2
        assert caret(ta) >= L2_START, "caret on line 2 before Home/End"
        key(ta, "Home")
        assert_eq(caret(ta), L2_START, "Home moves to the start of line 2, not byte 0")
        key(ta, "End")
        assert_eq(caret(ta), len(GOAL_TEXT), "End moves to the end of line 2 (buffer end here)")

        # Ctrl+Home / Ctrl+End reach the DOCUMENT boundaries, not the row.
        key(ta, "ArrowUp")  # caret onto line 1 (mid-buffer)
        assert L0_START < caret(ta) < len(GOAL_TEXT), "caret mid-buffer before Ctrl+Home/End"
        key_ctrl(ta, "Home")
        assert_eq(caret(ta), 0, "Ctrl+Home jumps to the document start (byte 0)")
        key_ctrl(ta, "End")
        assert_eq(caret(ta), len(GOAL_TEXT), "Ctrl+End jumps to the document end")
        # Ctrl+Shift+Home selects from the document end back to byte 0.
        ta.modifiers(ctrl=True, shift=True)
        key(ta, "Home")
        ta.modifiers()
        sel = selection(ta)
        assert sel is not None, "Ctrl+Shift+Home produces a selection"
        assert_eq(sel["start"], 0, "Ctrl+Shift+Home reaches the document start")
        assert_eq(sel["end"], len(GOAL_TEXT), "Ctrl+Shift+Home anchors at the document end")

        # Middle line: Home/End bracket exactly that line.
        key_ctrl(ta, "End")  # document end (L2), collapses the selection
        key(ta, "ArrowUp")  # -> L1
        assert L1_START <= caret(ta) <= L1_START + 2, "caret on the short middle line"
        key(ta, "Home")
        assert_eq(caret(ta), L1_START, "Home on the middle line moves to its start (byte 13)")
        key(ta, "End")
        assert_eq(caret(ta), L1_START + 2, "End on the middle line moves to its end (byte 15)")

        # ── Shift+Home / Shift+End extend the selection to the boundary
        key(ta, "Home")  # L1 start, byte 13
        key(ta, "ArrowRight")  # byte 14
        key_shift(ta, "End")
        sel = selection(ta)
        assert sel is not None, "Shift+End on the middle line produces a selection"
        assert_eq(sel["start"], L1_START + 1, "Shift+End anchors at the original caret (byte 14)")
        assert_eq(sel["end"], L1_START + 2, "Shift+End extends to the line end (byte 15)")

        key(ta, "End")  # collapse, L1 end byte 15
        key_shift(ta, "Home")
        sel = selection(ta)
        assert sel is not None, "Shift+Home produces a selection"
        assert_eq(sel["start"], L1_START, "Shift+Home reaches the line start (byte 13)")
        assert_eq(sel["end"], L1_START + 2, "Shift+Home anchors at the original caret (byte 15)")

        # ════════════════════════════════════════════════════════════
        #  (e) VISUAL Home / End is per-WRAPPED-ROW, not per logical line
        # ════════════════════════════════════════════════════════════
        replace_all(ta, LONG)
        assert_eq(text(ta).count("\n"), 0, "wrapped line has no newline")
        goto_start(ta)
        assert_eq(caret(ta), 0, "Home on the first visual row is byte 0")
        key(ta, "ArrowDown")
        row1_start = caret(ta)
        assert row1_start > 0, f"ArrowDown reaches the 2nd visual row (byte {row1_start} > 0)"
        # Move into the row, then Home returns to the VISUAL row start
        # (not byte 0 — proving Home is per visual row, not per line).
        key(ta, "ArrowRight")
        key(ta, "ArrowRight")
        key(ta, "Home")
        assert_eq(
            caret(ta),
            row1_start,
            "Home lands at the wrapped row start (not byte 0) — visual, not logical",
        )
        key(ta, "End")
        row1_end = caret(ta)
        assert row1_start < row1_end < len(LONG), (
            f"End lands at the wrapped row end, mid-buffer "
            f"(start {row1_start} < end {row1_end} < {len(LONG)})"
        )
        # The wrapped-row span carries no newline (it is a slice of the
        # single logical line).
        assert_eq(text(ta)[row1_start:row1_end].count("\n"), 0, "wrapped-row span has no newline")

        # Shift+Home on the wrapped row selects back to the row start.
        # Re-navigate cleanly (the prior End sat on the row1/row2 boundary
        # byte, which resolves onto row 2), then anchor a couple of glyphs
        # INTO row 1 so the selection is unambiguously within that row.
        goto_start(ta)
        key(ta, "ArrowDown")
        r1s = caret(ta)
        assert_eq(r1s, row1_start, "ArrowDown deterministically lands on the same row-1 start")
        key(ta, "ArrowRight")
        key(ta, "ArrowRight")  # mid-row
        mid = caret(ta)
        key_shift(ta, "Home")
        sel = selection(ta)
        assert sel is not None, "Shift+Home on a wrapped row selects"
        assert_eq(sel["start"], row1_start, "wrapped-row Shift+Home reaches the visual row start")
        assert_eq(sel["end"], mid, "wrapped-row Shift+Home anchors at the original mid-row caret")


if __name__ == "__main__":
    sys.exit(run_demo("R766 TextArea goal column + visual Home/End", body))
