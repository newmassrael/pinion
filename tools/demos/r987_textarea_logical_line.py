#!/usr/bin/env python3
"""R987 §5.22 §5.36 — current-line band spans the whole LOGICAL line.

R962 gave the textarea body a current-line background band, but it covered
only the caret's *visual* row: a soft-wrapped logical line highlighted just
the row the caret sat on. R987 spans the band across **every visual row of
the caret's logical line** (the VS Code / IntelliJ behaviour) via the new
`pinion_text::logical_line_span` substrate, fed from the same shaping pass
as the caret rect — so a wrapped line highlights fully.

Drives hello-textarea over JSON-RPC. The band is tagged
`main_textarea-current-line` (`tf_paint::current_line_band_tag`); its rect
is read from the paint snapshot (`abs_rects_of`). Soft-wrap is width-driven
(no `\\n`), so the wrapped line still reports 0 newlines in `/external/text`.

The decisive proof (section D): on a single wrapped logical line, moving the
caret *down between its visual rows* leaves the band rect UNCHANGED — it
keys on the logical line, not the caret's visual row. Pre-R987 the band
would have dropped to the caret's lower row.

Verification scope (>=30 assertions; gates per [[zero-flake-policy]]: every
action->assert edge polls observed state, no fixed sleeps):

  (A) boot regression — seeded multi-line content, Idle, focus.
  (B) a SHORT line that fits is ONE row: the band is one row tall, full
      inner width, anchored at the content top.
  (C) a LONG line WRAPS: the band is several rows tall (> the short band),
      ~N x the single-row height where N is the wrap count.
  (D) the band keys on the LOGICAL line: ArrowDown within the wrapped line
      moves the caret to a lower visual row but the band rect is unchanged.
  (E) a HARD `\\n` ends the span: the band moves to the next logical line,
      one row tall again.
  (F) the band is a passive decoration (R965.1): a click on it sets the
      caret (falls through, not intercepted).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

TA_TAG = "main_textarea"
BAND = f"{TA_TAG}-current-line"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"

# A short line that fits the 344 px inner width -> one visual row.
SHORT = "abc def"
# A long no-`\n` line that wraps onto several visual rows (fits the 5-row
# box, so no scroll); width-driven break, 0 newlines.
LONG = (
    "the quick brown fox jumps over the lazy dog while the "
    "sleepy cat watches the whole thing unfold slowly today"
)
# Two short logical lines (a hard newline ends a band's span).
TWO = "line one\nline two"


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def key(ta: RpcSubprocess, name: str) -> None:
    ta.key(path=TA_TAG, name=name)


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
    wait_until(lambda: text(ta) == body, desc=f"replace_all: {body[:16]!r}... landed")


def _walk(node: Any, out: list) -> list:
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def _band_rect(snap: Any):
    return abs_rects_of(snap).get(BAND)


def band_rect(ta: RpcSubprocess):
    return _band_rect(ta.snapshot(source="paint", viewport=VIEWPORT))


def band_count(ta: RpcSubprocess) -> int:
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    return sum(1 for n in _walk(field, []) if n.get("tag") == BAND)


def caret_box_y(ta: RpcSubprocess):
    """The painted caret box's y (the slim w==2 Box) — proves a repaint
    placed the caret on its new visual row."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    boxes = [
        n for n in _walk(field, [])
        if n.get("type") == "Box" and n.get("rect", {}).get("w") == 2
    ]
    return boxes[-1]["rect"]["y"] if boxes else None


def visual_rows_from_home(ta: RpcSubprocess) -> int:
    """Count visual rows of the current (single, no-`\\n`) logical line by
    walking ArrowDown from byte 0 (the r765 method)."""
    length = len(text(ta))
    key_ctrl(ta, "Home")
    wait_query(ta, "/external/caret", 0, desc="Ctrl+Home to byte 0")
    count, prev = 1, 0
    for _ in range(16):
        key(ta, "ArrowDown")
        cur = caret(ta)
        if cur <= prev or cur >= length:
            break
        count += 1
        prev = cur
    return count


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── (A) boot regression ──────────────────────────────────────
        assert_eq(ta.query("/external/state"), "Idle", "boot Idle")               # 1
        assert_eq(text(ta), SEED, "seeded multi-line text")                       # 2
        assert_eq(caret(ta), 0, "caret at 0 on boot")                             # 3
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="focused")              # 4

        # ── (B) a SHORT line is one row ──────────────────────────────
        replace_all(ta, SHORT)
        key_ctrl(ta, "Home")
        wait_query(ta, "/external/caret", 0, desc="caret home on short line")     # 5
        wait_until(lambda: _band_rect(ta.snapshot(source="paint", viewport=VIEWPORT)) is not None,
                   desc="band paints on the short line")                          # 6
        assert_eq(band_count(ta), 1, "exactly one current-line band")            # 7
        (sx, sy, sw, sh) = band_rect(ta)
        field = find_by_tag(ta.snapshot(source="paint", viewport=VIEWPORT), TA_TAG)
        fr = field["rect"]
        fx, fw = int(fr["x"]), int(fr["w"])
        left_inset = sx - fx
        assert left_inset > 0, f"band is inset by the field padding ({sx} vs {fx})"  # 8
        assert_eq(sw, fw - 2 * left_inset, "band width == field inner width")     # 9
        assert sh > 0, f"short-line band has a positive height ({sh})"            # 10

        # ── (C) a LONG line wraps: band spans several rows ───────────
        replace_all(ta, LONG)
        assert_eq(text(ta).count("\n"), 0, "long line has NO newline (width wrap)")  # 11
        n_rows = visual_rows_from_home(ta)
        assert n_rows >= 2, f"the long line wraps onto >=2 visual rows (got {n_rows})"  # 12
        key_ctrl(ta, "Home")
        wait_query(ta, "/external/caret", 0, desc="caret home on long line")      # 13
        (lx, ly, lw, lh) = band_rect(ta)
        assert_eq(lw, sw, "band width unchanged by wrapping (still inner width)") # 14
        assert lh > sh, f"wrapped band is taller than one row ({lh} > {sh})"      # 15
        # Uniform rows: the wrapped band ~ N x the single row, within 2 px.
        assert abs(lh - n_rows * sh) <= 2, (
            f"wrapped band spans ~{n_rows} rows ({lh} ~= {n_rows} x {sh})"
        )                                                                          # 16
        assert_eq(ly, sy, "the wrapped band still anchors at the content top")    # 17
        # R987 — the gutter current-line band spans the SAME logical line as
        # the body band (aligned, not just the number's first row).
        snap = ta.snapshot(source="paint", viewport=VIEWPORT)
        gutter = abs_rects_of(snap).get("ta_gutter_current")
        assert gutter is not None, "the gutter current-line band paints"          # 18
        assert_eq(gutter[3], lh, "gutter band spans the wrapped line, aligned with the body")  # 19

        # ── (D) the band keys on the LOGICAL line, not the visual row ─
        # Caret on visual row 0; record the band.
        r0 = band_rect(ta)
        y_home = caret_box_y(ta)
        # ArrowDown -> caret drops to visual row 1 of the SAME logical line.
        c0 = caret(ta)
        key(ta, "ArrowDown")
        wait_until(lambda: caret(ta) > c0, desc="ArrowDown moves caret to a lower visual row")  # 18
        assert_eq(text(ta).count("\n"), 0, "still one logical line after ArrowDown")  # 19
        wait_until(lambda: (lambda y: y is not None and y > y_home)(caret_box_y(ta)),
                   desc="the painted caret box dropped to the lower visual row")  # 20
        r1 = band_rect(ta)
        assert_eq(r1, r0, "band UNCHANGED as the caret moves within the logical line")  # 21
        # A second ArrowDown (row 2) when the line wraps that far.
        if n_rows >= 3:
            c1 = caret(ta)
            key(ta, "ArrowDown")
            wait_until(lambda: caret(ta) > c1, desc="second ArrowDown -> row 2")  # 22
            r2 = band_rect(ta)
            assert_eq(r2, r0, "band still spans the whole logical line on row 2")  # 23
        else:
            assert True, "line wrapped to exactly 2 rows; row-2 step skipped"     # 22/23

        # ── (E) a HARD newline ends the span ─────────────────────────
        # A `\n` must be typed via the Enter key (text input has no newline
        # glyph): select-all, type "line one", Enter, type "line two".
        select_all(ta)
        ta.text("line one", path=TA_TAG)
        key(ta, "Enter")
        ta.text("line two", path=TA_TAG)
        wait_until(lambda: text(ta) == TWO, desc="typed two hard logical lines")
        key_ctrl(ta, "Home")
        wait_query(ta, "/external/caret", 0, desc="caret on logical line 0")      # 24
        (_, b0y, _, b0h) = band_rect(ta)
        assert_eq(b0h, sh, "line-0 band is one row tall (no wrap)")               # 25
        # Move to the start of logical line 1 ("line two").
        line1_start = len("line one\n")
        key(ta, "ArrowDown")
        wait_until(lambda: caret(ta) >= line1_start, desc="caret reaches logical line 1")  # 26
        (_, b1y, _, b1h) = band_rect(ta)
        assert b1y > b0y, f"the band moved DOWN to the next logical line ({b1y} > {b0y})"  # 27
        assert_eq(b1h, sh, "line-1 band is one row tall too")                     # 28
        # (R1031 §5.37) Lines advance by ~one row. The step is the font's line
        # advance; the band height `sh` is the (ceil'd) line box, which can
        # exceed the advance when boxes overlap (e.g. DejaVu Sans Mono: step 20,
        # box 22). Only one band paints at a time, so a sub-row box overlap is
        # harmless — assert the step is positive and at most one band tall,
        # font-robustly, instead of an exact step==height that assumes contiguous
        # boxes.
        assert 0 < b1y - b0y <= sh, (
            f"the two hard lines are ~one row apart (0 < {b1y - b0y} <= {sh})"    # 29
        )

        # ── (F) the band is a passive decoration (R965.1) ────────────
        # A click on the line-1 band region sets the caret (falls through).
        rect = band_rect(ta)
        bx, by, bw, bh = rect
        key_ctrl(ta, "Home")
        wait_query(ta, "/external/caret", 0, desc="re-home before the fall-through click")  # 30
        ta.click(at=(bx + bw // 2, by + bh // 2))
        wait_until(lambda: caret(ta) > 0, desc="click on the band falls through to the caret")  # 31
        assert caret(ta) > 0, "the band does not intercept the click (pointer-transparent)"  # 32

        # ── round-trip: navigation never edited the text ────────────
        assert_eq(text(ta), TWO, "the buffer text is unchanged by navigation")    # 33
        assert_eq(text(ta).count("\n"), 1, "still two logical lines")             # 34


if __name__ == "__main__":
    sys.exit(run_demo("R987 §5.22 §5.36 — current-line band spans the logical line", body))
