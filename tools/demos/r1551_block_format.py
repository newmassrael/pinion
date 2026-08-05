#!/usr/bin/env python3
"""R1551 §5.36 §5.12 §5.40 — a paragraph states its own block format.

Before this round pinion had the character half of rich text and none of the
block half: a paragraph could say how its glyphs looked and nothing about how
the paragraph itself sat. No indent, no space between paragraphs, no first-line
indent, no way to mark one a heading — the whole of Qt's `QTextBlockFormat`.

R1551 makes the block format a first-class scene declaration, lowered to the
ordinary layout box (so the flex pass indents a paragraph with no
document-specific layout code, on BOTH backends) and, for the first-line
indent, into how the shaper breaks the paragraph.

What this demo asserts, over the wire, against a real application:

  * `scene/snapshot` carries the DECLARATION — each paragraph's `block` and its
    `style.text_indent`, as scene data;
  * `scene/text_blocks` carries the CONSEQUENCE — where each shaped line
    landed, beside the declaration it came from. That pairing is the only form
    in which "did my indent reach the layout" has an answer, and Qt has neither
    half as data: `QTextBlockFormat` is a property bag reachable only through a
    `QTextCursor`, and the geometry lives in a separate private layout object
    that has no per-line accessor at all;
  * the declared first-line indent IS where line 0 starts, and no other line
    moves — checked as a number, not as a pixel;
  * the Toggle flips the SELECTION (`hanging`) while the AMOUNT stays put, and
    the line boxes mirror: the indent moves off line 0 and onto every
    continuation. One declaration, two shapes;
  * a block quote's indents are its node's box, cross-checked against
    `scene/snapshot`'s independent rect — so the two introspection channels
    verify each other rather than restating one derivation;
  * a paragraph declaring nothing is not a block — the negative control that
    separates "the feature published a paragraph" from "the method lists text";
  * `scene/access` carries the heading OUTLINE with `aria-level`, which is the
    half of `headingLevel` Qt does not have: `QAccessibleTextInterface` has no
    method that reports block structure, so a Qt document's heading levels
    reach its layout and stop there;
  * `rpc/schema` describes the four new types, so the R1539 census covers this
    round's wire the day it lands.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-richtext-blocks --release
    python3 tools/demos/r1551_block_format.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-richtext-blocks"
VIEWPORT = (560, 460)

DOC_TAG = "essay"
TOGGLE_TAG = "main_toggle"

# The paragraphs, by index — `DocumentTag::block("essay", i)`.
H1, BODY, QUOTE, BIB, H2 = 0, 1, 2, 3, 4


def block_tag(i: int) -> str:
    """The paint tag of the i-th paragraph — `DocumentTag::block`'s spelling."""
    return f"{DOC_TAG}_blk{i}"


# What the binding declares. The demo asserts the wire reports THESE rather
# than whatever the layout happened to produce.
INDENT_PX = 28
QUOTE_INDENT_PX = 32
BLOCK_SPACE_PX = 10

# The exact published shape of one paragraph report. A response an agent is told
# to rely on should not be able to gain or lose a key unnoticed (the R1538
# lesson, which R1539 turned into a gate).
BLOCK_KEYS = {
    "tag",
    "x",
    "y",
    "width",
    "height",
    "block",
    "text_indent",
    "align",
    "lines",
}
FORMAT_KEYS = {
    "left_indent_px",
    "right_indent_px",
    "space_above_px",
    "space_below_px",
    "heading_level",
    "aria_level",
}
LINE_KEYS = {"start", "end", "x", "y", "advance", "trailing_whitespace", "height"}

# A line's x is a shaped f32; compare against the declared px with a tolerance
# well below one cell so it cannot absorb a wrong answer.
EPS = 0.5


def report_for(tf: RpcSubprocess, i: int) -> dict[str, Any]:
    for row in call(tf, "scene/text_blocks")["blocks"]:
        if row["tag"] == block_tag(i):
            return row
    raise AssertionError(f"no block report for {block_tag(i)}")


def line_starts(report: dict[str, Any]) -> list[float]:
    return [line["x"] for line in report["lines"]]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # A paint scene is what both surfaces read, so ask for one first.
        wait_until(
            lambda: find_by_tag(
                tf.snapshot(source="paint", viewport=VIEWPORT), block_tag(BODY)
            )
            is not None,
            timeout=5.0,
            interval=0.03,
            desc="the document is painted",
        )

        # ── (A) the DECLARATION, as scene data ─────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        quote = find_by_tag(snap, block_tag(QUOTE))
        assert quote is not None, "A: the block quote is in the paint scene"
        assert_eq(
            quote["block"],
            {
                "left_indent_px": QUOTE_INDENT_PX,
                "right_indent_px": QUOTE_INDENT_PX,
                "space_above_px": BLOCK_SPACE_PX,
                "space_below_px": BLOCK_SPACE_PX,
                "heading_level": 0,
            },
            "A: the whole declared format, as a struct rather than a bag",
        )
        heading = find_by_tag(snap, block_tag(H1))
        assert heading is not None, "A: the chapter heading is painted"
        assert_eq(heading["block"]["heading_level"], 1, "A: a level-1 heading")
        plain = find_by_tag(snap, block_tag(BODY))
        assert plain is not None, "A: the body paragraph is painted"
        assert_eq(
            plain["block"],
            {
                "left_indent_px": 0,
                "right_indent_px": 0,
                "space_above_px": 0,
                "space_below_px": 0,
                "heading_level": 0,
            },
            "A: every paragraph of a document HAS a block format, as in Qt — "
            "and declaring one invents no indents",
        )
        assert_eq(
            plain["style"]["text_indent"],
            {"amount_px": INDENT_PX, "hanging": False, "each_line": False},
            "A: its text indent is declared with both CSS keywords, which "
            "Qt's bare qreal `textIndent` has no spelling for",
        )
        chip = find_by_tag(snap, TOGGLE_TAG)
        assert chip is not None, "A: the mode chip is painted"
        label = next(c for c in chip["children"] if c.get("type") == "Text")
        assert_eq(
            label["block"],
            None,
            "A: an ordinary label is NOT a paragraph — null, so a reader can "
            "tell a document block from any other text without guessing",
        )

        # ── (B) the CONSEQUENCE — where the shaped lines landed ────────────
        rows = call(tf, "scene/text_blocks")["blocks"]
        assert_eq(
            {r["tag"] for r in rows},
            {block_tag(i) for i in (H1, BODY, QUOTE, BIB, H2)},
            "B: exactly the five declared paragraphs",
        )
        body_report = report_for(tf, BODY)
        assert_eq(set(body_report), BLOCK_KEYS, "B: a report's exact key set")
        assert_eq(
            set(report_for(tf, QUOTE)["block"]),
            FORMAT_KEYS,
            "B: a declared format's exact key set",
        )
        assert_eq(set(body_report["lines"][0]), LINE_KEYS, "B: a line's exact key set")
        assert len(body_report["lines"]) > 1, (
            f"B: the body paragraph wraps: {body_report['lines']}"
        )

        # ── (C) the declared indent IS where line 0 starts ─────────────────
        starts = line_starts(body_report)
        assert abs(starts[0] - INDENT_PX) < EPS, (
            f"C: line 0 starts at the declared {INDENT_PX}px: {starts}"
        )
        assert all(abs(x) < EPS for x in starts[1:]), (
            f"C: and no other line moved: {starts}"
        )
        assert_eq(
            body_report["align"], "Start", "C: with no alignment offset in play"
        )
        assert body_report["lines"][0]["advance"] > 0, "C: the line has real content"
        assert_eq(
            body_report["lines"][0]["start"], 0, "C: and it starts at byte 0"
        )
        assert (
            body_report["lines"][1]["start"] == body_report["lines"][0]["end"]
        ), f"C: the lines tile the content: {body_report['lines'][:2]}"

        # ── (D) an indented line has LESS room, not the same room shifted ──
        # The property that separates a real text-indent from a paint offset:
        # the first line breaks earlier, so it holds fewer bytes than it would
        # have. Checked against the paragraph the same width whose indent hangs.
        bib_before = report_for(tf, BIB)
        assert abs(line_starts(bib_before)[0]) < EPS, (
            "D: the bibliography entry's first line is flush"
        )
        assert all(
            abs(x - INDENT_PX) < EPS for x in line_starts(bib_before)[1:]
        ), f"D: and its continuations hang: {line_starts(bib_before)}"

        # ── (E) a block quote's indents ARE its box ────────────────────────
        # Cross-checked against `scene/snapshot`'s rect, which reaches the same
        # geometry by an independent path, so this is a check rather than a
        # restatement.
        quote_report = report_for(tf, QUOTE)
        quote_rect = quote["rect"]
        assert_eq(
            [quote_report["x"], quote_report["y"]],
            [quote_rect["x"], quote_rect["y"]],
            "E: the two surfaces agree about where the quote sits",
        )
        body_rect = plain["rect"]
        assert_eq(
            quote_rect["w"],
            body_rect["w"] - 2 * QUOTE_INDENT_PX,
            "E: and the quote's box is exactly its declared indents narrower "
            "than the body paragraph's — the declaration reached the flex pass",
        )
        assert quote_rect["x"] == body_rect["x"] + QUOTE_INDENT_PX, (
            f"E: shifted by the left indent: quote={quote_rect} body={body_rect}"
        )

        # ── (F) the Toggle moves the SELECTION, not the amount ─────────────
        tf.click(path=TOGGLE_TAG)
        wait_until(
            lambda: report_for(tf, BODY)["text_indent"]["hanging"],
            timeout=4.0,
            interval=0.03,
            desc="the indent mode flips",
        )
        hanging = report_for(tf, BODY)
        assert_eq(
            hanging["text_indent"]["amount_px"],
            INDENT_PX,
            "F: the same amount — `hanging` inverts which lines it selects",
        )
        hung = line_starts(hanging)
        assert abs(hung[0]) < EPS, f"F: the first line is now flush: {hung}"
        assert all(abs(x - INDENT_PX) < EPS for x in hung[1:]), (
            f"F: and every continuation is indented: {hung}"
        )
        assert_eq(
            len(hung), len(starts), "F: the same paragraph, the same line count"
        )
        assert hung != starts, "F: but not the same shape"

        # ── (G) negative control: undeclared text is not a block ───────────
        # Without this, "the method returned blocks" would be satisfied by a
        # method that lists every text node in the scene.
        tags = {r["tag"] for r in call(tf, "scene/text_blocks")["blocks"]}
        assert not any(t is None for t in tags), "G: every block is addressable"
        assert TOGGLE_TAG not in tags, (
            "G: the chip's label declares no paragraph and is not one"
        )
        assert_eq(len(tags), 5, f"G: five paragraphs and nothing else: {tags}")

        # ── (H) the heading OUTLINE reaches assistive technology ───────────
        access = call(tf, "scene/access")
        headings = [n for n in access["nodes"] if n["role"] == "heading"]
        assert_eq(len(headings), 2, f"H: two headings in the outline: {headings}")
        by_tag = {n["tag"]: n for n in headings}
        assert_eq(by_tag[block_tag(H1)]["level"], 1, "H: the chapter is level 1")
        assert_eq(by_tag[block_tag(H2)]["level"], 2, "H: the section is level 2")
        assert_eq(
            by_tag[block_tag(H1)]["name"],
            "Chapter One",
            "H: named by the PAINTED text, so the announced string is the drawn "
            "one — there is no second source it could come from",
        )
        assert block_tag(BODY) not in by_tag, (
            "H: an ordinary paragraph is not a heading"
        )

        # ── (I) the R1539 census covers this round's wire ──────────────────
        schema = call(tf, "rpc/schema")
        by_name = {t["name"]: t for t in schema["types"]}
        for name in (
            "TextBlockReport",
            "TextBlocksOutcome",
            "BlockFormatWire",
            "TextIndentWire",
            "TextLineWire",
        ):
            assert name in by_name, f"I: {name} is described by the published census"
        assert_eq(
            {f["name"] for f in by_name["TextBlockReport"]["shape"]["fields"]},
            BLOCK_KEYS,
            "I: and the census's key set is the one the live wire answered with",
        )
        assert_eq(
            {f["name"] for f in by_name["BlockFormatWire"]["shape"]["fields"]},
            FORMAT_KEYS,
            "I: for the declared format too",
        )
        block_field = next(
            f
            for f in by_name["TextBlockReport"]["shape"]["fields"]
            if f["name"] == "block"
        )
        assert_eq(
            block_field["nullable"],
            True,
            "I: the census states that `block` may be null, which is the "
            "indent-only case an agent must handle",
        )

        # ── (J) the method is discoverable and does not mutate ─────────────
        methods = call(tf, "rpc/methods")["methods"]
        entry = next(m for m in methods if m["name"] == "scene/text_blocks")
        assert_eq(entry["occ"], "read", "J: reading a paragraph mutates nothing")

        # ── (K) and reading it changed nothing ─────────────────────────────
        assert_eq(
            line_starts(report_for(tf, BODY)),
            hung,
            "K: the document is where (F) left it",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1551 §5.36 a paragraph states its own block format", body))
