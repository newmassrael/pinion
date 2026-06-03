#!/usr/bin/env python3
"""R765 §5.22 §5.36 — textarea soft-wrap at width.

R764 landed multi-line text editing where lines are split by explicit
`\\n`. R765 enables soft-wrap: [`TextFieldStyle::m3_multiline`] sets
`soft_wrap = true`, so the `field_shaping` SSOT threads the field's inner
width (`field_w - 2*field_pad` = 360 - 16 = 344 px) to every geometry
consumer (paint / caret / hit-test / vertical nav). A long line with NO
`\\n` then breaks onto additional *visual* lines, and parley resolves
wrapped-line caret/selection/point geometry for free.

The proof here is pure DATA over RPC, with a contrast experiment:

- A SHORT no-newline line (fits in 344 px) is ONE visual line:
  `ArrowDown` from its start clamps to its end. No wrap when it fits.
- A LONG no-newline line wraps: `ArrowDown` from its start lands at the
  start of the *next visual line* (a byte strictly inside the string,
  not the end), and repeated `ArrowDown` walks N>1 visual lines before
  reaching the end. A click on the lower visual-line band lands mid
  string. Selection across a wrap boundary extends one visual line.

Because the break is driven by WIDTH, not a `\\n`, the wrapped line still
reports 0 newlines in `/external/text` — the wrap is purely visual.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r765_textarea_softwrap.py
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo

TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"  # 33 bytes, 3 lines
SETTLE = 0.06

# A short line that fits within the 344 px inner width (one visual line).
SHORT = "abc def"
# A long line with no '\n' that exceeds 344 px and must wrap onto
# several visual lines. Width-driven break, so 0 newlines.
LONG = (
    "the quick brown fox jumps over the lazy dog while the "
    "sleepy cat watches the whole thing unfold slowly today"
)
# Much longer: wraps onto ~7 visual lines, taller than the 5-row box, so
# the content overflows the viewport and must clip + scroll-to-caret.
VERYLONG = (
    "the quick brown fox jumps over the lazy dog while the sleepy cat "
    "watches the whole thing unfold very slowly today and then the dog "
    "decides to chase its own tail around the yard until it gets dizzy "
    "and finally lies down in the warm afternoon sunshine to rest now"
)


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


def scroll_geometry(ta: RpcSubprocess):
    """Pull the multi-line field's `Scene::Scroll` node + caret box from
    the paint scene. Returns a dict with the scroll offset, the viewport
    height, the scrolled content height, and the caret box's content-space
    y (or None if not focused). All in logical pixels."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    nodes = _walk(field, [])
    scroll = next(n for n in nodes if n.get("type") == "Scroll")
    vp = scroll["viewport"]
    content = scroll["content"]
    # The caret is the slim (caret_width = 2 px) Box inside the content.
    caret_boxes = [
        n for n in _walk(content, [])
        if n.get("type") == "Box" and n.get("rect", {}).get("w") == 2
    ]
    caret_y = caret_boxes[-1]["rect"]["y"] if caret_boxes else None
    caret_h = caret_boxes[-1]["rect"]["h"] if caret_boxes else 0
    return {
        "offset_y": int(scroll.get("offset_y", 0)),
        "viewport_h": int(vp["h"]),
        "content_h": int(content["rect"]["h"]),
        "caret_y": caret_y,
        "caret_h": caret_h,
    }


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def selection(ta: RpcSubprocess):
    return ta.query("/external/selection")


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def key(ta: RpcSubprocess, name: str) -> None:
    ta.key(path=TA_TAG, name=name)
    time.sleep(SETTLE)


def key_ctrl(ta: RpcSubprocess, name: str) -> None:
    """R766 — Home/End are now visual-line-relative; Ctrl+Home / Ctrl+End
    reach the DOCUMENT boundaries (byte 0 / text.len()). This demo
    navigates to the buffer ends, so it uses the Ctrl variants."""
    ta.modifiers(ctrl=True)
    key(ta, name)
    ta.modifiers()


def select_all(ta: RpcSubprocess) -> None:
    ta.modifiers(ctrl=True)
    key(ta, "a")
    ta.modifiers()


def replace_all(ta: RpcSubprocess, body: str) -> None:
    """Select all, then type `body` — the first keystroke replaces the
    selection (W3C type-to-replace), the rest append, so the field ends
    holding exactly `body`."""
    select_all(ta)
    ta.text(body, path=TA_TAG)
    time.sleep(SETTLE)


def visual_line_count_from_start(ta: RpcSubprocess) -> int:
    """Count visual lines from column 0 by walking `ArrowDown` from the
    buffer start. Each `ArrowDown` that lands at a *new visual-line start*
    (a byte strictly greater than the previous and strictly less than the
    buffer end) is one more line; the move that clamps to the buffer end
    means we fell off the last line (not a new line). Starting at byte 0
    is itself the first visual line, so the count begins at 1."""
    length = len(text(ta))
    key_ctrl(ta, "Home")
    assert_eq(caret(ta), 0, "Ctrl+Home moves the caret to the buffer start")
    count = 1
    prev = 0
    for _ in range(16):  # generous cap; LONG wraps into a handful of lines
        key(ta, "ArrowDown")
        cur = caret(ta)
        if cur <= prev or cur >= length:
            break  # no advance, or clamped to buffer end -> last line reached
        count += 1
        prev = cur
    return count


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── boot regression: R764 seeded multi-line content ──────────
        assert_eq(ta.query("/external/state"), "Idle", "initial state Idle")
        assert_eq(text(ta), SEED, "seeded multi-line text")
        assert_eq(text(ta).count("\n"), 2, "seed has 2 newlines (3 lines)")
        assert_eq(caret(ta), 0, "caret at 0 on boot")
        assert_eq(selection(ta), None, "no selection on boot")

        ta.request("focus/set", {"tag": TA_TAG})
        time.sleep(0.05)
        assert_eq(ta.query("/external/state"), "Focused", "focused after focus/set")

        # ── Ctrl+A select-all covers the whole seeded buffer ─────────
        select_all(ta)
        sel = selection(ta)
        assert sel is not None, "Ctrl+A produces a selection"
        assert_eq(sel["start"], 0, "select-all anchors at byte 0")
        assert_eq(sel["end"], len(SEED), "select-all reaches the buffer end")

        # ── CONTRAST 1: a SHORT line that FITS is one visual line ────
        replace_all(ta, SHORT)
        assert_eq(text(ta), SHORT, "short line replaced the buffer")
        assert_eq(text(ta).count("\n"), 0, "short line has no newline")
        # One visual line: ArrowDown from the start clamps to the end.
        key_ctrl(ta, "Home")
        assert_eq(caret(ta), 0, "caret at start of the short line")
        key(ta, "ArrowDown")
        assert_eq(
            caret(ta),
            len(SHORT),
            "ArrowDown on a single (unwrapped) visual line clamps to its end",
        )
        assert_eq(
            visual_line_count_from_start(ta),
            1,
            "the short line that fits occupies exactly one visual line",
        )

        # ── CONTRAST 2: a LONG line WRAPS onto several visual lines ──
        replace_all(ta, LONG)
        assert_eq(text(ta), LONG, "long line replaced the buffer")
        assert_eq(text(ta).count("\n"), 0, "long line has NO newline (wrap is width-driven)")

        key_ctrl(ta, "Home")
        assert_eq(caret(ta), 0, "caret at start of the long line")
        key(ta, "ArrowDown")
        c_down = caret(ta)
        assert c_down > 0, f"ArrowDown leaves the first visual line (caret {c_down} > 0)"
        assert c_down < len(LONG), (
            f"ArrowDown lands at the start of the next visual line, "
            f"not the buffer end (caret {c_down} < {len(LONG)})"
        )

        vlines = visual_line_count_from_start(ta)
        assert vlines >= 3, (
            f"the long line wraps onto >=3 visual lines (got {vlines}); "
            f"width-driven break with 0 newlines"
        )

        # Ctrl+End reaches the true buffer end regardless of visual wrapping.
        key_ctrl(ta, "End")
        assert_eq(caret(ta), len(LONG), "Ctrl+End reaches the buffer end past the wraps")

        # ── hit-test resolves against the wrapped layout ─────────────
        (fx, fy, fw, fh) = field_rect(ta)

        def line_y(row: int) -> float:
            # pad(8) + line_height(~25) * row + half-line(12)
            return fy + 8 + 25 * row + 12

        # Click the FIRST visual line near its left edge -> near byte 0.
        ta.click(at=(fx + 2, line_y(0)))
        time.sleep(SETTLE)
        c_top = caret(ta)
        assert c_top <= 2, f"click on visual line 0 left edge lands near byte 0 (got {c_top})"

        # Click the SECOND visual line -> a byte strictly inside the
        # single logical line (the wrapped portion), not the end.
        ta.click(at=(fx + 2, line_y(1)))
        time.sleep(SETTLE)
        c_mid = caret(ta)
        assert 0 < c_mid < len(LONG), (
            f"click on visual line 1 lands mid-string (byte {c_mid} in 1..{len(LONG) - 1})"
        )
        assert c_mid > c_top, "lower visual line resolves to a later byte than the top line"

        # ── selection across a wrap boundary ─────────────────────────
        key_ctrl(ta, "Home")
        assert_eq(selection(ta), None, "selection cleared before shift-nav")
        ta.modifiers(shift=True)
        key(ta, "ArrowDown")
        ta.modifiers()
        sel = selection(ta)
        assert sel is not None, "Shift+ArrowDown across a wrap produces a selection"
        assert_eq(sel["start"], 0, "selection anchored at the line start")
        assert sel["end"] > 0, f"selection extends across the wrap (end {sel['end']} > 0)"
        assert sel["end"] < len(LONG), "one Shift+ArrowDown extends just one visual line"
        assert_eq(caret(ta), sel["end"], "caret (focus) sits at the selection end")
        # The selected span is a contiguous slice of the single logical line.
        span = text(ta)[sel["start"]:sel["end"]]
        assert_eq(span.count("\n"), 0, "wrapped-line selection holds no newline")
        assert LONG.startswith(span), "selection is the leading slice of the long line"

        # ── drag-select the whole wrapped block ──────────────────────
        ta.click(at=(fx + 2, line_y(0)))
        time.sleep(SETTLE)
        ta.drag(
            from_at=(fx + 2, line_y(0)),
            to_at=(fx + fw - 2, line_y(2)),
            steps=12,
        )
        time.sleep(SETTLE)
        sel = selection(ta)
        assert sel is not None, "multi-visual-line drag produces a selection"
        assert sel["start"] <= 1, f"drag selection starts near the line start (got {sel['start']})"
        assert sel["end"] > c_mid, (
            f"drag across 3 visual lines reaches past visual line 1 (end {sel['end']} > {c_mid})"
        )

        # ── CONTRAST 3: content taller than the box clips + scrolls ──
        # VERYLONG wraps onto more visual lines than the 5-row box shows,
        # so the field must clip the overflow to its inner box and scroll
        # vertically to keep the caret visible (scroll-to-caret).
        replace_all(ta, VERYLONG)
        assert_eq(text(ta).count("\n"), 0, "very long line is still one logical line")

        key_ctrl(ta, "Home")
        g = scroll_geometry(ta)
        assert g["content_h"] > g["viewport_h"], (
            f"wrapped content ({g['content_h']}px) overflows the viewport "
            f"({g['viewport_h']}px) — clip + scroll required"
        )
        assert_eq(g["offset_y"], 0, "at the document start the content is scrolled to the top (offset 0)")
        # Caret on screen = viewport_top + caret_content_y - offset; at the
        # top that is just caret_y. It must sit inside the viewport.
        assert g["caret_y"] is not None, "caret painted while focused"
        assert 0 <= g["caret_y"] - g["offset_y"] <= g["viewport_h"], (
            "caret is visible within the viewport at the document start"
        )

        key_ctrl(ta, "End")
        g = scroll_geometry(ta)
        assert g["offset_y"] > 0, (
            f"at the document end the content is scrolled down to reveal the last line "
            f"(offset {g['offset_y']} > 0)"
        )
        max_scroll = g["content_h"] - g["viewport_h"]
        assert g["offset_y"] <= max_scroll, (
            f"scroll offset clamped to content bounds ({g['offset_y']} <= {max_scroll})"
        )
        # Caret (last line) projected through the scroll must stay inside
        # the visible viewport — the whole point of scroll-to-caret.
        end_offset = g["offset_y"]
        caret_screen = g["caret_y"] - end_offset
        assert 0 <= caret_screen <= g["viewport_h"], (
            f"caret stays visible after scrolling (screen y {caret_screen} "
            f"in 0..{g['viewport_h']})"
        )
        assert caret_screen + g["caret_h"] <= g["viewport_h"] + 2, (
            "caret bottom does not spill below the visible box"
        )

        # Canonical scroll-into-view (NOT pin-to-bottom): one ArrowUp from
        # End moves the caret UP within the viewport without scrolling the
        # document — the offset stays put because the caret is still
        # visible. A pin-to-bottom rule would instead keep the caret glued
        # to the bottom edge and scroll the document on every move.
        key(ta, "ArrowUp")
        g2 = scroll_geometry(ta)
        assert_eq(
            g2["offset_y"],
            end_offset,
            "ArrowUp within the viewport does NOT scroll (caret-into-view, not pin-to-bottom)",
        )
        assert g2["caret_y"] is None or (g2["caret_y"] - g2["offset_y"]) < caret_screen, (
            "the caret moved UP within the viewport (smaller screen y) without a scroll"
        )

        # hit-test reads the SAME stored scroll offset paint applied: at
        # End (scrolled), a click near the TOP of the viewport lands on a
        # scrolled-in line, not the buffer start (byte 0). Without the
        # offset the click would resolve to the first line.
        key_ctrl(ta, "End")
        time.sleep(SETTLE)
        assert scroll_geometry(ta)["offset_y"] > 0, "scrolled at the document end before hit-test"
        (hx, hy, hw, hh) = field_rect(ta)
        ta.click(at=(hx + 5, hy + 12))  # near the top of the visible box
        time.sleep(SETTLE)
        c_hit = caret(ta)
        assert c_hit > 10, (
            f"click near the viewport top at scroll lands on a scrolled-in line, "
            f"not the buffer start (byte {c_hit} > 10) — hit-test honours the offset"
        )

        # Scrolling back to the top releases the offset (caret-driven).
        key_ctrl(ta, "Home")
        assert_eq(
            scroll_geometry(ta)["offset_y"],
            0,
            "returning the caret to the top scrolls the content back to 0",
        )

        # ── soft-wrap is purely visual: round-trip preserves the text ─
        replace_all(ta, LONG)
        key_ctrl(ta, "End")
        assert_eq(text(ta), LONG, "the buffer text is unchanged by wrapping/navigation")
        assert_eq(text(ta).count("\n"), 0, "still a single logical line (no newline injected)")


if __name__ == "__main__":
    sys.exit(run_demo("R765 TextArea soft-wrap", body))
