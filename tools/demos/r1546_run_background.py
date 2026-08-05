#!/usr/bin/env python3
"""R1546 §5.36 §5.12 — a text run states its background (Qt `setBackground`).

Before this round a `TextStyle` had a foreground and no background. The
terminal cell had carried both since the §5.41 grid arm existed, so one
document was paintable with a highlight in a terminal and not as text — and
what the missing extension point produced instead was visible in the paint
layer, which hand-rolls FOUR band kinds (selection / find-match / current-line
/ IME-preedit), each an absolute-positioned box with its own fill function, and
a comment conceding all four bodies were byte-identical.

R1546 makes the background a property of the RUN, the way Qt's
`QTextCharFormat::setBackground` and CSS's inline `background-color` are, and
derives the painted band from the SHAPED LAYOUT through the very function the
selection band already uses — so a highlight and a selection over the same
bytes cannot disagree about where they are.

What this demo asserts, over the wire, against a real application:

  * `scene/snapshot` carries the DECLARATION — `bg_color` on the run, `null`
    on every style that declares none, which is a different fact from a
    transparent one;
  * `scene/text_backgrounds` carries the CONSEQUENCE — where the band was
    actually painted. This is the half Qt has no accessor for at all: the rect
    is computed inside the private `QTextLayout::draw` and discarded, so a Qt
    application re-derives it from `cursorToX` and hopes its second
    implementation agrees with the painter's;
  * the published band lands inside the node it belongs to, checked against
    that node's rect from an INDEPENDENT surface (`scene/snapshot`), so the
    two introspection channels are cross-verified rather than self-consistent;
  * the band's WCAG contrast is published, and the Toggle drives it ACROSS the
    4.5 body-text bar — 11:1 down to 1.3:1 — while the geometry does not move
    by a pixel. Qt will paint any brush behind any pen and say nothing;
  * a run declaring NO background inside a base style that has one punches a
    hole: two bands with a gap, because a `StyleRun` carries a fully-resolved
    style and a byte has exactly one background;
  * text declaring nothing contributes no band — the negative control that
    separates "the feature published a band" from "the method lists text";
  * `rpc/schema` describes the three new types, so the R1539 census covers
    this round's wire the day it lands;
  * `background_builds` proves the bands are derived once and replayed: the
    property is a count, because a rebuild and a replay paint identical pixels.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-richtext-background --release
    python3 tools/demos/r1546_run_background.py
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

EXAMPLE = "hello-richtext-background"
VIEWPORT = (520, 260)

HIT_TAG = "search_hit"
CLAUSE_TAG = "clause"
TOGGLE_TAG = "main_toggle"

# The byte ranges the binding declares. The demo asserts the wire reports THESE
# rather than whatever the shaper happened to split the text into.
HIT_RANGE = (16, 19)
HOLE_RANGE = (9, 13)
CLAUSE_LEN = len("over the lazy dog")

READABLE = {"r": 0xFF, "g": 0xF1, "b": 0x76, "a": 0xFF}
UNREADABLE = {"r": 0x2A, "g": 0x1E, "b": 0x6E, "a": 0xFF}
HIT_INK = {"r": 0x11, "g": 0x11, "b": 0x11, "a": 0xFF}

# The exact published shape of one band. A response an agent is told to rely on
# should not be able to gain or lose a key unnoticed (the R1538 lesson, which
# R1539 turned into a gate).
BAND_KEYS = {
    "tag",
    "start",
    "end",
    "x",
    "y",
    "width",
    "height",
    "color",
    "fg_color",
    "contrast",
}

# WCAG 2.x small-text bar.
BODY_TEXT_BAR = 4.5


def bands(tf: RpcSubprocess) -> list[dict[str, Any]]:
    return call(tf, "scene/text_backgrounds")["bands"]


def band_for(rows: list[dict[str, Any]], tag: str, start: int) -> dict[str, Any]:
    for row in rows:
        if row["tag"] == tag and row["start"] == start:
            return row
    raise AssertionError(f"no band for {tag} at byte {start} in {rows}")


def style_of(node: dict[str, Any]) -> dict[str, Any]:
    return node["style"]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # A paint scene is what both surfaces read, so ask for one first.
        wait_until(
            lambda: find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), HIT_TAG)
            is not None,
            timeout=5.0,
            interval=0.03,
            desc="the search-hit line is painted",
        )

        # ── (A) the DECLARATION, as scene data ─────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        hit = find_by_tag(snap, HIT_TAG)
        assert hit is not None, "A: the hit line is in the paint scene"
        assert_eq(len(hit["runs"]), 1, "A: one run declares the highlight")
        run = hit["runs"][0]
        assert_eq([run["start"], run["end"]], list(HIT_RANGE), "A: the matched range")
        assert_eq(run["style"]["bg_color"], READABLE, "A: the declared highlight")
        assert_eq(run["style"]["fg_color"], HIT_INK, "A: the ink drawn on it")
        assert_eq(
            style_of(hit)["bg_color"],
            None,
            "A: an undeclared background is null — the unset brush, which is "
            "not the same fact as a transparent one",
        )

        # ── (B) the CONSEQUENCE — where it was painted ─────────────────────
        rows = bands(tf)
        assert_eq(
            {r["tag"] for r in rows},
            {HIT_TAG, CLAUSE_TAG},
            "B: exactly the two nodes that declare a background",
        )
        hit_band = band_for(rows, HIT_TAG, HIT_RANGE[0])
        assert_eq(
            set(hit_band) - {"contrast_note"},
            BAND_KEYS,
            "B: a band's exact key set",
        )
        assert_eq(
            [hit_band["start"], hit_band["end"]], list(HIT_RANGE), "B: its range"
        )
        assert_eq(hit_band["color"], READABLE, "B: the colour actually painted")
        assert hit_band["width"] > 0, f"B: a positive extent, got {hit_band['width']}"
        assert hit_band["height"] > 0, f"B: a positive height, got {hit_band['height']}"

        # ── (C) cross-check against an INDEPENDENT surface ─────────────────
        # The band's own numbers could be internally consistent and still wrong.
        # `scene/snapshot` reports the node's rect through a different path, so
        # containment is a real check rather than a restatement.
        rect = hit["rect"]
        assert rect["x"] <= hit_band["x"], f"C: band starts inside the node {hit_band}"
        assert hit_band["x"] + hit_band["width"] <= rect["x"] + rect["w"], (
            f"C: and ends inside it: band={hit_band} rect={rect}"
        )
        # Vertically the band is the LINE BOX, whose natural top can sit a
        # pixel above the box the layout engine gave the node (parley reports
        # it; the glyph ink lives there too). So the check is registration
        # within one line, not containment — the property that would actually
        # be violated by a band derived for the wrong node or the wrong row.
        assert abs(hit_band["y"] - rect["y"]) <= hit_band["height"], (
            f"C: the band is on this node's line: band={hit_band} rect={rect}"
        )
        assert hit_band["height"] <= rect["h"] + 2, "C: no taller than the line"
        # A highlight over 3 of 25 characters is a fraction of the line, not the
        # whole box — the assertion that separates "the band was measured" from
        # "the band is the node rect with a colour".
        assert hit_band["width"] < rect["w"] // 2, (
            f"C: three characters of twenty-five: {hit_band['width']} of {rect['w']}"
        )

        # ── (D) the contrast, published ────────────────────────────────────
        readable_ratio = hit_band["contrast"]
        assert readable_ratio is not None, "D: an opaque background states its ratio"
        assert readable_ratio >= BODY_TEXT_BAR, (
            f"D: marker pen clears the WCAG body-text bar: {readable_ratio}"
        )
        assert "contrast_note" not in hit_band, "D: nothing to explain when it is known"
        assert_eq(hit_band["fg_color"], HIT_INK, "D: the pair the ratio is of")

        # ── (E) drive it across the bar; the GEOMETRY must not move ────────
        before = [hit_band["x"], hit_band["y"], hit_band["width"], hit_band["height"]]
        tf.click(path=TOGGLE_TAG)
        wait_until(
            lambda: band_for(bands(tf), HIT_TAG, HIT_RANGE[0])["color"] == UNREADABLE,
            timeout=4.0,
            interval=0.03,
            desc="the palette bit repaints the highlight",
        )
        indigo = band_for(bands(tf), HIT_TAG, HIT_RANGE[0])
        unreadable_ratio = indigo["contrast"]
        assert unreadable_ratio is not None, "E: still opaque, still stated"
        assert unreadable_ratio < BODY_TEXT_BAR, (
            f"E: indigo does NOT clear the bar: {unreadable_ratio}"
        )
        assert readable_ratio > unreadable_ratio * 4, (
            f"E: and it is not a rounding difference: {readable_ratio} vs "
            f"{unreadable_ratio}"
        )
        assert_eq(
            [indigo["x"], indigo["y"], indigo["width"], indigo["height"]],
            list(before),
            "E: a colour change is not a geometry change — the band is derived "
            "from the SHAPED layout, which the palette does not touch",
        )
        assert_eq(indigo["fg_color"], HIT_INK, "E: the ink is what stayed fixed")

        # ── (F) a run with no background punches a hole ────────────────────
        clause_bands = [r for r in bands(tf) if r["tag"] == CLAUSE_TAG]
        assert_eq(len(clause_bands), 2, f"F: the wash is split in two: {clause_bands}")
        left, right = clause_bands
        assert_eq([left["start"], left["end"]], [0, HOLE_RANGE[0]], "F: up to the word")
        assert_eq(
            [right["start"], right["end"]],
            [HOLE_RANGE[1], CLAUSE_LEN],
            "F: and resumes after it",
        )
        assert left["x"] + left["width"] <= right["x"], (
            f"F: with a real gap where the word is: {clause_bands}"
        )
        assert_eq(left["y"], right["y"], "F: both on the one line")
        assert_eq(left["color"], right["color"], "F: one declaration, two bands")

        # ── (G) negative control: text declaring nothing gets no band ──────
        # Without this, "the method returned bands" would be satisfied by a
        # method that lists every text node in the scene.
        assert_eq(
            [r for r in bands(tf) if r["tag"] not in (HIT_TAG, CLAUSE_TAG)],
            [],
            "G: the title, the chip label and the status line declare no "
            "background and contribute none",
        )
        titles = [
            n
            for n in snap["children"]
            if n.get("type") == "Text" and n.get("tag") is None
        ]
        assert titles, "G: there really are undeclared text nodes to have skipped"

        # ── (H) the R1539 census covers this round's wire ──────────────────
        schema = call(tf, "rpc/schema")
        by_name = {t["name"]: t for t in schema["types"]}
        for name in ("TextBackgroundBand", "TextBackgroundsOutcome", "ColorWire"):
            assert name in by_name, f"H: {name} is described by the published census"
        described = {f["name"] for f in by_name["TextBackgroundBand"]["shape"]["fields"]}
        assert_eq(
            described,
            BAND_KEYS | {"contrast_note"},
            "H: and the census's key set is the one the live wire answered with",
        )
        contrast_field = next(
            f
            for f in by_name["TextBackgroundBand"]["shape"]["fields"]
            if f["name"] == "contrast"
        )
        assert_eq(
            contrast_field["nullable"],
            True,
            "H: the census states that contrast may be null, which is the "
            "translucent case an agent must handle",
        )

        # ── (I) derived once, replayed ─────────────────────────────────────
        # A count, not a flag: a rebuilt band list and a replayed one paint
        # identical pixels, so nothing else separates them.
        stats = call(tf, "scene/text_cache_stats")
        assert "background_builds" in stats, "I: the counter is published"
        settled = stats["background_builds"]
        for _ in range(5):
            bands(tf)
        assert_eq(
            call(tf, "scene/text_cache_stats")["background_builds"],
            settled,
            "I: five more reads of an unchanged scene derive nothing further",
        )
        assert settled > 0, f"I: and something really was derived: {settled}"

        # ── (J) the method is discoverable ─────────────────────────────────
        methods = call(tf, "rpc/methods")["methods"]
        entry = next(m for m in methods if m["name"] == "scene/text_backgrounds")
        assert_eq(entry["occ"], "read", "J: reading where a band landed mutates nothing")

        # ── (K) and reading it changed nothing ─────────────────────────────
        assert_eq(
            band_for(bands(tf), HIT_TAG, HIT_RANGE[0])["color"],
            UNREADABLE,
            "K: the scene is where (E) left it",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1546 §5.36 a text run states its background", body))
