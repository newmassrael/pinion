#!/usr/bin/env python3
"""R962 §5.22 §5.36 — body current-line background band for the textarea.

Drives hello-textarea over JSON-RPC. R957 gave the *gutter* a current-line
highlight band (`ta_gutter_current`, spanning the narrow line-number column).
R962 adds the *body* sibling: `tf_paint::view_field` paints a faint full-width
Accent wash across the editor row the caret sits on — the VS Code "current
line" affordance — gated by the new `TextFieldStyle::current_line_alpha` (off
by default, opted into here). The band is tagged `main_textarea-current-line`
(`tf_paint::current_line_band_tag`) so the AI side reads which row is active
straight from the painted frame.

What this proves:

  (A) **the body band exists and is unique**: exactly one band per frame,
      tagged off the field's own tag;
  (B) **it spans the full editor width**: inset from the field box by an equal
      margin on both sides (the field padding), far wider than the gutter band;
  (C) **it tracks the caret line**: its top edge is share-for-share equal to the
      R957 gutter band's top — both resolve the same caret line from the same
      shaped layout — and it drops monotonically as the caret moves down;
  (D) **it follows a keyboard / RPC caret move**, not just pointer clicks.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r962_textarea_current_line.py

>= 30 assertions.
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
    wait_snap,
)

WIN = (480, 320)
EXT = "/external"
FIELD = "main_textarea"
BODY = f"{FIELD}-current-line"  # tf_paint::current_line_band_tag(FIELD)
GBAND = "ta_gutter_current"     # R957 gutter current-line band

# "alpha\nbeta\ngamma\ndelta\nepsilon" — five logical lines.
DOC = "alpha\nbeta\ngamma\ndelta\nepsilon"
LINE_START = [0, 6, 11, 17, 23]


def gnum(n: int) -> str:
    """The composite paint tag of the gutter's 1-based line number `n`."""
    return f"{FIELD}#gl{n}"


def _band_count(snap: Any) -> int:
    """Count body current-line bands in the paint tree (must be exactly one)."""
    n = 0

    def walk(node: Any) -> None:
        nonlocal n
        if not isinstance(node, dict):
            return
        if node.get("tag") == BODY:
            n += 1
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(snap)
    return n


def _check_band_geometry(snap: Any, line: int) -> tuple[int, int, int, int]:
    """Assert the body band spans the field's inner width at the caret row and
    sits on the same row as the R957 gutter band. Returns the band rect."""
    rects = abs_rects_of(snap)
    field = rects[FIELD]
    band = rects[BODY]
    gband = rects[GBAND]
    fx, _, fw, _ = field
    bx, _, bw, _ = band
    # (B) full-width: symmetric inset (the field padding) on both sides.
    left_inset = bx - fx
    right_inset = (fx + fw) - (bx + bw)
    assert left_inset > 0, f"line {line}: band is inset from the field, not full-bleed ({band} vs {field})"
    assert_eq(left_inset, right_inset, f"line {line}: band insets are symmetric (= field padding)")
    assert_eq(bw, fw - 2 * left_inset, f"line {line}: band width == field inner width")
    # ... and far wider than the narrow gutter band.
    assert band[2] > gband[2], f"line {line}: body band ({band[2]}px) wider than gutter band ({gband[2]}px)"
    # (C) same row as the gutter band — both resolve the caret line.
    assert_eq(band[1], gband[1], f"line {line}: body band top == gutter band top")
    assert_eq(band[3], gband[3], f"line {line}: body band height == gutter band height")
    return band


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ed:
        ed.intervene(f"{EXT}/text", DOC)
        wait_query(ed, f"{EXT}/text", DOC, desc="buffer set")
        assert_eq(ed.query(f"{EXT}/line_count"), 5, "five logical lines")

        # (A) the body band is present from boot (always-on, like the gutter
        #     band — a current-line affordance, not a focus affordance).
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, BODY) is not None,
            viewport=WIN,
            desc="body current-line band painted",
        )
        assert_eq(_band_count(snap), 1, "exactly one body band at boot")

        # (C) caret at line 1 -> band sits at the content top (field origin +
        #     padding), deterministic regardless of font metrics.
        ed.invoke(f"{EXT}/go-to-line", 1)
        wait_query(ed, f"{EXT}/caret", LINE_START[0], desc="caret at line 1")
        snap = wait_snap(
            ed,
            lambda s: abs_rects_of(s).get(BODY, (0, 0, 0, 0))[1]
            == abs_rects_of(s).get(GBAND, (0, 0, 0, 1))[1],
            viewport=WIN,
            desc="band aligns with the gutter band at line 1",
        )
        rects = abs_rects_of(snap)
        field = rects[FIELD]
        band1 = rects[BODY]
        pad = band1[0] - field[0]
        assert_eq(band1[1], field[1] + pad, "line 1 band top == field top + padding")

        # (A)+(B)+(C) walk every line; the band tracks the caret and stays a
        # single full-width row.
        tops: list[int] = []
        for n in range(1, 6):
            ed.click(path=gnum(n))
            wait_query(
                ed, f"{EXT}/caret", LINE_START[n - 1],
                desc=f"click gutter {n} -> caret at line {n}",
            )
            snap = wait_snap(
                ed,
                lambda s, ln=n: find_by_tag(s, BODY) is not None
                and abs_rects_of(s).get(BODY, (0, 0, 0, 0))[1]
                == abs_rects_of(s).get(GBAND, (0, 0, 0, 1))[1],
                viewport=WIN,
                desc=f"body band tracks the caret to line {n}",
            )
            assert_eq(_band_count(snap), 1, f"exactly one body band at line {n}")
            band = _check_band_geometry(snap, n)
            tops.append(band[1])

        # (C) the band drops monotonically as the caret moves down the document.
        assert all(tops[i] < tops[i + 1] for i in range(4)), f"band top increases per line: {tops}"

        # (D) a keyboard / RPC caret move (not a pointer click) moves the band.
        ed.invoke(f"{EXT}/go-to-line", 2)
        wait_query(ed, f"{EXT}/caret", LINE_START[1], desc="go-to-line 2 (RPC) moves the caret")
        snap = wait_snap(
            ed,
            lambda s: abs_rects_of(s).get(BODY, (0, 0, 0, 0))[1]
            == abs_rects_of(s).get(GBAND, (0, 0, 0, 1))[1],
            viewport=WIN,
            desc="body band follows the RPC caret move to line 2",
        )
        assert_eq(_band_count(snap), 1, "still exactly one body band after the keyboard-path move")
        _check_band_geometry(snap, 2)
        # The band climbed back up from line 5 to line 2.
        assert abs_rects_of(snap)[BODY][1] < tops[4], "band moved up from line 5 to line 2"


if __name__ == "__main__":
    sys.exit(run_demo("R962 §5.22 §5.36 — body current-line band", body))
