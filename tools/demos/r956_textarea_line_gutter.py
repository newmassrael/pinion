#!/usr/bin/env python3
"""R956 §5.36 §5.22 — line-number gutter for the multi-line textarea.

Drives hello-textarea over JSON-RPC. The textarea could edit, soft-wrap, select,
navigate and rich-format multi-line text, but it had no line-number gutter — the
table-stakes affordance of every code editor. R956 adds the foundational
substrate `pinion_text::visual_line_metrics` (per-VISUAL-line geometry from the
shaped parley `Layout`: each row's `y` / `height` + a `starts_logical_line`
flag), the `tf_paint::field_visual_lines` reader (same shaped layout `view_field`
paints, so the metrics align row-for-row with the glyphs), and a gutter in the
binding that mirrors the field's own `Scene::Scroll`.

The gutter is the first external consumer of `tf_paint::field_scroll_offset`
(now `pub`): it scrolls in lock-step with the field so the numbers track the
scrolled text rows.

  (A) the gutter shows one right-aligned number per LOGICAL line, aligned with
      the field's content origin, evenly spaced, left of the field box;
  (B) editing re-derives the gutter (more / fewer lines -> more / fewer numbers);
  (C) a soft-wrapped long line (no `\\n`) keeps ONE number for the whole
      paragraph (only the first visual row is a logical-line start) — and when it
      wraps past the viewport the field scrolls, proving the wrap, with the
      gutter scroll offset matching the field's (lock-step);
  (D) hard-line scroll: a tall buffer + go-to-line scrolls the field; the gutter
      offset equals the field offset, and returns to 0 together;
  (E) an empty buffer still shows line "1".

The gutter geometry is read from the PAINT snapshot (`abs_rects_of` re-derives
the scroll-translated window-absolute rects), so every alignment claim is
grounded in the rendered frame, not inferred ([[introspection-from-paint-not-screen]]).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r956_textarea_line_gutter.py

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
PAD = 8  # TextFieldStyle::m3_multiline field_pad
EXT = "/external"
GUT = "ta_gutter"  # the gutter container box (still tagged here)
FIELD = "main_textarea"


def gnum(n: int) -> str:
    """R959 — the gutter's 1-based line number `n` is tagged in the field's own
    composite send namespace (`gutter_line_sub_tag(FIELD, n)`), not under the
    `ta_gutter` box, so a click routes to the field's `send` wire."""
    return f"{FIELD}#gl{n}"


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{EXT}/text", text)
    wait_query(ed, f"{EXT}/text", text, desc="buffer set")


def _logical_count(text: str) -> int:
    return text.count("\n") + 1


def _gutter_tags(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    return {t: r for t, r in abs_rects_of(snap).items() if t.startswith(FIELD + "#gl")}


def _num_content(snap: Any, n: int) -> Optional[str]:
    node = find_by_tag(snap, gnum(n))
    return node.get("content") if node else None


def _scroll_offset(snap: Any, container_tag: str) -> Optional[int]:
    """The vertical scroll offset of the first `Scroll` under a tagged box."""

    def find_scroll(node: Any) -> Optional[dict]:
        if not isinstance(node, dict):
            return None
        if node.get("type") == "Scroll":
            return node
        for child in node.get("children") or []:
            hit = find_scroll(child)
            if hit is not None:
                return hit
        content = node.get("content")
        if isinstance(content, dict):
            return find_scroll(content)
        return None

    box = find_by_tag(snap, container_tag)
    if box is None:
        return None
    scroll = find_scroll(box)
    return None if scroll is None else int(scroll.get("offset_y", 0))


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ed:
        # ── (A) boot: 3-line seed shows numbers 1/2/3, aligned ──────────
        seed = "first line\nsecond line\nthird line"
        _set_text(ed, seed)
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(3)) is not None,
            viewport=WIN,
            desc="gutter shows three numbers",
        )
        nums = _gutter_tags(snap)
        assert_eq(len(nums), 3, "three logical lines -> three gutter numbers")
        assert_eq(_num_content(snap, 1), "1", "first number reads '1'")
        assert_eq(_num_content(snap, 2), "2", "second number reads '2'")
        assert_eq(_num_content(snap, 3), "3", "third number reads '3'")
        assert find_by_tag(snap, gnum(4)) is None, "no fourth number for three lines"

        rects = abs_rects_of(snap)
        gutter_box = rects[GUT]
        field_box = rects[FIELD]
        # The gutter sits to the LEFT of the field, butted against it.
        assert (
            gutter_box[0] + gutter_box[2] <= field_box[0]
        ), f"gutter ({gutter_box}) is left of the field ({field_box})"
        # Same box top (a top-aligned Row) and equal height.
        assert_eq(gutter_box[1], field_box[1], "gutter and field share the box top")
        assert_eq(gutter_box[3], field_box[3], "gutter and field share the box height")
        # Number 1 aligns with the field's content origin (box top + padding).
        g1y = nums[gnum(1)][1]
        assert abs(g1y - (field_box[1] + PAD)) <= 4, (
            f"number 1 (y {g1y}) aligns with the field content top "
            f"({field_box[1] + PAD})"
        )
        # Numbers increase in y and are evenly spaced (one per row).
        ys = [nums[gnum(n)][1] for n in (1, 2, 3)]
        assert ys[0] < ys[1] < ys[2], f"numbers descend the gutter in order: {ys}"
        step1, step2 = ys[1] - ys[0], ys[2] - ys[1]
        assert step1 > 0 and abs(step1 - step2) <= 2, (
            f"line numbers are evenly spaced (steps {step1} vs {step2})"
        )
        # Content fits five rows: no scroll, gutter and field both at 0.
        assert_eq(_scroll_offset(snap, FIELD), 0, "field is unscrolled at boot")
        assert_eq(_scroll_offset(snap, GUT), 0, "gutter is unscrolled at boot")

        # ── (B) editing re-derives the gutter ───────────────────────────
        _set_text(ed, "a\nb\nc\nd\ne\nf")
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(6)) is not None,
            viewport=WIN,
            desc="gutter grows to six numbers",
        )
        assert_eq(len(_gutter_tags(snap)), 6, "six lines -> six numbers")
        assert_eq(_num_content(snap, 6), "6", "the sixth number reads '6'")

        _set_text(ed, "only\ntwo")
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(3)) is None
            and find_by_tag(s, gnum(2)) is not None,
            viewport=WIN,
            desc="gutter shrinks to two numbers",
        )
        assert_eq(len(_gutter_tags(snap)), 2, "two lines -> two numbers")
        assert find_by_tag(snap, gnum(3)) is None, "the third number is gone"

        # ── (C) a soft-wrapped paragraph keeps ONE number ───────────────
        # No '\n', long enough to wrap past five rows so the field scrolls.
        wrapped = "the quick brown fox jumps over the lazy dog " * 6
        assert "\n" not in wrapped, "the wrap fixture is a single logical line"
        _set_text(ed, wrapped)
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(1)) is not None,
            viewport=WIN,
            desc="the wrapped paragraph shows its single number",
        )
        assert_eq(_logical_count(wrapped), 1, "the fixture is one logical line")
        assert_eq(len(_gutter_tags(snap)), 1, "a wrapped line keeps ONE gutter number")
        assert find_by_tag(snap, gnum(2)) is None, "no second number for one logical line"

        # Move the caret to the end: the wrapped content overflows five rows,
        # so the field scrolls — proving the single line wrapped — and the
        # gutter scrolls with it.
        ed.intervene(f"{EXT}/caret", len(wrapped))
        wait_query(ed, f"{EXT}/caret", len(wrapped), desc="caret at the buffer end")
        snap = wait_snap(
            ed,
            lambda s: (_scroll_offset(s, FIELD) or 0) > 0,
            viewport=WIN,
            desc="the wrapped line scrolled the field",
        )
        wrap_field_off = _scroll_offset(snap, FIELD)
        wrap_gut_off = _scroll_offset(snap, GUT)
        assert wrap_field_off and wrap_field_off > 0, (
            f"the wrapped paragraph scrolled the field ({wrap_field_off}) — "
            "it occupies more than five visual rows"
        )
        assert abs(wrap_field_off - (wrap_gut_off or 0)) <= 1, (
            f"the gutter offset ({wrap_gut_off}) tracks the field offset "
            f"({wrap_field_off}) in lock-step"
        )
        # Still exactly one number through the scroll.
        assert_eq(len(_gutter_tags(snap)), 1, "the wrapped line stays one number while scrolled")

        # ── (D) hard-line scroll lock-step ──────────────────────────────
        tall = "\n".join(f"row {n}" for n in range(1, 21))  # 20 lines
        _set_text(ed, tall)
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(20)) is not None,
            viewport=WIN,
            desc="twenty numbers for twenty lines",
        )
        assert_eq(len(_gutter_tags(snap)), 20, "twenty lines -> twenty numbers")
        assert_eq(_num_content(snap, 1), "1", "first number reads '1'")
        assert_eq(_num_content(snap, 20), "20", "twentieth number reads '20'")

        # Jump to the last line: the field scrolls to reveal it.
        assert_eq(ed.invoke(f"{EXT}/go-to-line", 20), 20, "go-to-line 20 echoes line 20")
        snap = wait_snap(
            ed,
            lambda s: (_scroll_offset(s, FIELD) or 0) > 0,
            viewport=WIN,
            desc="the field scrolled to the last line",
        )
        f_off = _scroll_offset(snap, FIELD)
        g_off = _scroll_offset(snap, GUT)
        assert f_off and f_off > 0, f"field scrolled to reveal line 20 (offset {f_off})"
        assert abs(f_off - (g_off or 0)) <= 1, (
            f"gutter offset ({g_off}) == field offset ({f_off}) under hard-line scroll"
        )

        # Back to the top: both return to 0 together.
        assert_eq(ed.invoke(f"{EXT}/go-to-line", 1), 1, "go-to-line 1 echoes line 1")
        snap = wait_snap(
            ed,
            lambda s: (_scroll_offset(s, FIELD) or 0) == 0,
            viewport=WIN,
            desc="the field scrolled back to the top",
        )
        assert_eq(_scroll_offset(snap, FIELD), 0, "field back at the top")
        assert_eq(_scroll_offset(snap, GUT), 0, "gutter back at the top with it")

        # ── (E) an empty buffer still numbers line 1 ────────────────────
        _set_text(ed, "")
        snap = wait_snap(
            ed,
            lambda s: find_by_tag(s, gnum(1)) is not None
            and find_by_tag(s, gnum(2)) is None,
            viewport=WIN,
            desc="an empty buffer shows one number",
        )
        assert_eq(len(_gutter_tags(snap)), 1, "empty buffer -> a single gutter number")
        assert_eq(_num_content(snap, 1), "1", "the empty buffer's line is numbered '1'")
        assert_eq(ed.query(f"{EXT}/caret"), 0, "caret at the start of the empty buffer")


if __name__ == "__main__":
    sys.exit(run_demo("R956 §5.36 §5.22 — textarea line-number gutter", body))
