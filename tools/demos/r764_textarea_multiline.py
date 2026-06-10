#!/usr/bin/env python3
"""R764 §5.22 §5.36 — multi-line TextField (textarea).

The single-line TextField stack (R56.1.b caret, R762 click-caret, R763
pointer selection) generalises to a textarea on top of three R764
substrate pieces, all driven here over RPC as DATA:

- Per-line selection bands (`selection_rects_for_range` → parley
  `Selection::geometry`): a selection that spans hard newlines paints one
  band per visual line. Verified by a drag from line 0 to line 2
  producing a multi-line `selection_range`.
- Vertical caret navigation (`byte_offset_for_line_move` → parley
  `Selection::move_lines`): ArrowUp/Down move between lines holding the
  column. Verified by column-0 navigation landing on each line start.
- `Enter` inserts a newline (the one binding divergence from a
  single-line field).

Seed text = "first line\\nsecond line\\nthird line":
  line 0 = bytes 0..10, '\\n' @10, line 1 = 11..22, '\\n' @22,
  line 2 = 23..33  (total 33 bytes, 3 lines).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r764_textarea_multiline.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"  # 33 bytes, 3 lines


def field_rect(ta: RpcSubprocess):
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, TA_TAG)
    assert node is not None, "textarea present in paint scene"
    r = node["rect"]
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def selection(ta: RpcSubprocess):
    return ta.query("/external/selection")


def key(ta: RpcSubprocess, name: str) -> None:
    ta.key(path=TA_TAG, name=name)


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── boot: seeded multi-line content ──────────────────────────
        assert_eq(ta.query("/external/state"), "Idle", "initial state Idle")
        assert_eq(ta.query("/external/text"), SEED, "seeded multi-line text")
        assert_eq(ta.query("/external/text").count("\n"), 2, "seed has 2 newlines (3 lines)")
        assert_eq(caret(ta), 0, "caret at 0 on boot")
        assert_eq(selection(ta), None, "no selection on boot")

        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="focused after focus/set")

        # ── vertical caret navigation (column 0 → line starts) ───────
        key(ta, "ArrowDown")
        assert_eq(caret(ta), 11, "ArrowDown from line 0 col 0 -> line 1 start (byte 11)")
        key(ta, "ArrowDown")
        assert_eq(caret(ta), 23, "ArrowDown -> line 2 start (byte 23)")
        key(ta, "ArrowUp")
        assert_eq(caret(ta), 11, "ArrowUp -> back to line 1 start (byte 11)")
        key(ta, "ArrowUp")
        assert_eq(caret(ta), 0, "ArrowUp -> line 0 start (byte 0)")
        key(ta, "ArrowUp")
        assert_eq(caret(ta), 0, "ArrowUp from first line clamps to 0")

        # horizontal arrows still work (substrate path), no vertical drift
        key(ta, "ArrowRight")
        assert_eq(caret(ta), 1, "ArrowRight advances one byte on the same line")
        key(ta, "ArrowLeft")
        assert_eq(caret(ta), 0, "ArrowLeft returns to 0")

        (fx, fy, fw, fh) = field_rect(ta)
        # line band centres: pad(8) + line_height(25) * row + 12
        def line_y(row: int) -> float:
            return fy + 8 + 25 * row + 12

        # ── click-to-position lands on the clicked line ──────────────
        ta.click(at=(fx + 2, line_y(1)))
        wait_until(
            lambda: 11 <= caret(ta) <= 22,
            desc="click on line 1 lands on line 1 (byte in 11..22)",
        )
        ta.click(at=(fx + 2, line_y(2)))
        wait_until(
            lambda: 23 <= caret(ta) <= 33,
            desc="click on line 2 lands on line 2 (byte in 23..33)",
        )

        # ── Shift+ArrowDown extends the selection one line ───────────
        ta.click(at=(fx + 2, line_y(0)))  # caret to line 0 start
        wait_until(
            lambda: caret(ta) <= 1,
            desc="click re-homes the caret to line 0 start",
        )
        assert_eq(selection(ta), None, "selection cleared before shift-nav")
        c_before = caret(ta)
        ta.modifiers(shift=True)
        key(ta, "ArrowDown")
        ta.modifiers()  # release Shift
        sel = selection(ta)
        assert sel is not None, "Shift+ArrowDown produces a selection"
        assert sel["start"] == c_before, "selection anchored at the pre-nav caret"
        assert sel["end"] > 10, f"selection extends onto line 1 (end {sel['end']} > 10)"
        assert_eq(caret(ta), sel["end"], "caret (focus) sits at the extended selection end")

        # ── multi-line drag selection (line 0 -> line 2) ─────────────
        ta.click(at=(fx + 2, line_y(0)))  # clear selection
        wait_until(
            lambda: selection(ta) is None,
            desc="pre-drag click clears the selection",
        )
        ta.drag(
            from_at=(fx + 2, line_y(0)),
            to_at=(fx + fw - 2, line_y(2)),
            steps=10,
        )
        sel = wait_until(
            lambda: selection(ta),
            desc="multi-line drag produces a selection",
        )
        assert sel["start"] <= 1, f"drag selection starts near line 0 start (got {sel['start']})"
        assert sel["end"] >= 23, f"drag selection reaches line 2 (end {sel['end']} >= 23)"
        span = ta.query("/external/text")[sel["start"]:sel["end"]]
        assert span.count("\n") >= 2, f"selection spans >=2 newlines (got {span.count(chr(10))})"
        assert span.startswith("first"), f"selection begins at line 0 text (got {span[:8]!r})"

        # ── Enter inserts a newline ──────────────────────────────────
        ta.click(at=(fx + 2, line_y(0)))  # caret to line 0
        wait_until(
            lambda: caret(ta) <= 10,
            desc="click re-homes the caret to line 0",
        )
        c0 = caret(ta)
        before_text = ta.query("/external/text")
        before_lines = before_text.count("\n") + 1
        key(ta, "Enter")
        after_text = ta.query("/external/text")
        assert_eq(after_text.count("\n") + 1, before_lines + 1, "Enter adds one line")
        assert_eq(caret(ta), c0 + 1, "caret advances past the inserted newline")
        assert after_text[c0] == "\n", "the inserted byte at the caret is a newline"

        # ── typing after newline inserts on the new line ─────────────
        assert_eq(ta.invoke("/external/key", "Z"), True, "type Z after the newline")
        wait_until(
            lambda: caret(ta) == c0 + 2,
            desc="caret advances past the typed char",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R764 TextArea multi-line", body))
