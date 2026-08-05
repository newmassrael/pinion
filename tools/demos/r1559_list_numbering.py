#!/usr/bin/env python3
"""R1559 §5.36 §5.12 §5.40 — an item is numbered by its place among its siblings.

Everything else a list has can be written by hand: the indent is a margin, the
bullet is a glyph. The NUMBER cannot, because it is not a property of the item
— it is a property of the item's position. Insert one item and every item after
it renumbers; nest one and the inner sequence restarts while the outer one
carries on underneath. That is what R1559 derives, and what this demo asserts
against a real application, over the wire.

  * `scene/text_lists` publishes the structure: one row per list, each with its
    items in order, the marker each was given, the counter it was numbered
    with, and where that marker was PAINTED. Qt has no peer for any of it — a
    `QTextList` exists only inside a `QTextDocument`, `itemNumber()` and
    `itemText()` are in-process C++ calls, there is no accessor that enumerates
    a document's lists, and a marker's position is computed inside the private
    `QTextDocumentLayout` and discarded;
  * the Toggle INSERTS a step into the middle of a procedure, and the markers
    of every later step move while the earlier ones do not. Nothing in the
    binding states a number, so this is the derivation and not an author;
  * a nested list restarts at its own first marker and does not interrupt the
    list it is nested in — checked by the outer list's numbering resuming
    beneath it;
  * an upper-roman list crosses 3999, where Roman numerals stop having a
    standard form. Qt's `itemText()` answers "?" there and the value is gone;
    CSS Counter Styles Level 3 renders through the fallback style, so the item
    reads `4000.` and the wire names `Decimal` as the notation that wrote it —
    the DECLARED style is still reported as `UpperRoman`, so the two are
    distinguishable;
  * a bullet is real text with a tag and a box. In Qt it is a painted ellipse,
    so there is nothing to read;
  * `scene/access` carries WAI-ARIA `list` / `listitem` with `aria-posinset` /
    `aria-setsize` / `aria-level`. Qt's `QAccessibleTextInterface` has no method
    that reports block structure at all;
  * `scene/snapshot` carries the same derivation on each paragraph, so the two
    introspection channels check each other rather than restating one;
  * a paragraph outside every list is not an item — the negative control that
    separates "the feature published a list" from "the method lists text";
  * `rpc/schema` describes the three new types, so the R1539 census covers this
    round's wire the day it lands.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-richtext-list --release
    python3 tools/demos/r1559_list_numbering.py
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

EXAMPLE = "hello-richtext-list"
VIEWPORT = (600, 520)

DOC_TAG = "guide"
TOGGLE_TAG = "main_toggle"

# What the binding declares. The demo asserts the wire reports THESE rather
# than whatever the layout happened to produce.
STEP_INDENT_PX = 34
BULLET_INDENT_PX = 22
ROMAN_INDENT_PX = 116
ROMAN_START = 3999

# The paragraphs, by their text — addressed by CONTENT rather than by index,
# because inserting a step shifts every later index and that shift is the
# behaviour under test.
H1_TEXT = "Assembly"
INTRO_TEXT = "Work through the steps in order."
STEP_1 = "Unpack the parts and lay them out."
PART_A = "two long bolts"
PART_B = "one hex key"
STEP_INSERTED = "Check them against the packing list."
STEP_BOLT = "Bolt the frame together, finger tight."
STEP_TIGHTEN = "Tighten in a star pattern."
ROMAN_A = "the last numeral there is a form for"
ROMAN_B = "one past it, written the way CSS says"

BULLET = "•"

# The exact published shape of one list row and one item. A response an agent
# is told to rely on should not be able to gain or lose a key unnoticed (the
# R1538 lesson, which R1539 turned into a gate).
LIST_KEYS = {
    "tag",
    "parent_tag",
    "level",
    "style",
    "start",
    "number_prefix",
    "number_suffix",
    "suffix_is_default",
    "indent_px",
    "count",
    "x",
    "y",
    "width",
    "height",
    "items",
}
ITEM_KEYS = {
    "tag",
    "marker_tag",
    "position",
    "ordinal",
    "marker",
    "rendered_as",
    "fell_back",
    "marker_x",
    "marker_y",
    "marker_width",
    "marker_height",
}
PLACEMENT_KEYS = {
    "list_tag",
    "parent_list_tag",
    "level",
    "ordinal",
    "position",
    "count",
    "marker",
    "rendered_as",
    "format",
}


def lists(tf: RpcSubprocess) -> list[dict[str, Any]]:
    return call(tf, "scene/text_lists")["lists"]


def text_of_tag(snap: Any, tag: str) -> str:
    node = find_by_tag(snap, tag)
    assert node is not None, f"no painted node tagged {tag}"
    return node.get("content", "")


def markers_by_text(tf: RpcSubprocess) -> dict[str, str]:
    """Every list item's painted text mapped to its painted marker.

    Joined through the item's paragraph tag, which is the key `scene/snapshot`,
    `scene/text_lists` and `scene/access` all address the same object by.
    """
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    out: dict[str, str] = {}
    for row in lists(tf):
        for item in row["items"]:
            out[text_of_tag(snap, item["tag"])] = item["marker"]
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # A paint scene is what every surface below reads, so ask for one first.
        wait_until(
            lambda: find_by_tag(
                tf.snapshot(source="paint", viewport=VIEWPORT), f"{DOC_TAG}_lst0"
            )
            is not None,
            timeout=5.0,
            interval=0.03,
            desc="the document is painted",
        )

        # ── (A) the STRUCTURE: three lists, nested as declared ─────────────
        rows = lists(tf)
        assert_eq(len(rows), 3, f"A: three lists on screen: {[r['tag'] for r in rows]}")
        steps, parts, roman = rows
        assert_eq(set(steps), LIST_KEYS, "A: a list row's exact key set")
        assert_eq(set(steps["items"][0]), ITEM_KEYS, "A: an item's exact key set")
        assert_eq(steps["tag"], f"{DOC_TAG}_lst0", "A: lists are addressable")
        assert_eq(steps["style"], "Decimal", "A: the procedure is numbered")
        assert_eq(steps["level"], 0)
        assert_eq(steps["parent_tag"], None, "A: and is top level")
        assert_eq(steps["count"], 3, "A: three steps before the insert")
        assert_eq(steps["indent_px"], STEP_INDENT_PX, "A: its declared gutter")
        assert_eq(parts["style"], "Disc", "A: the parts are bulleted")
        assert_eq(parts["level"], 1, "A: one level in")
        assert_eq(
            parts["parent_tag"],
            steps["tag"],
            "A: and the nesting is walkable without re-deriving it from `level`",
        )
        assert_eq(parts["count"], 2, "A: the parts are their OWN set")
        assert_eq(parts["indent_px"], BULLET_INDENT_PX, "A: with its own gutter")
        assert_eq(roman["style"], "UpperRoman")
        assert_eq(roman["start"], ROMAN_START, "A: started where it declared")
        assert_eq(roman["indent_px"], ROMAN_INDENT_PX)

        # ── (B) the suffix's default belongs to the STYLE ──────────────────
        assert_eq(
            steps["number_suffix"],
            ".",
            "B: a counter is followed by a full stop by default",
        )
        assert steps["suffix_is_default"], (
            "B: and the wire says the default is where it came from — Qt spells "
            "that distinction as a null QString, which no serialization carries"
        )
        assert_eq(parts["number_suffix"], "", "B: a bullet is not")
        assert parts["suffix_is_default"], "B: also by default"
        assert_eq(steps["number_prefix"], "", "B: and nothing precedes a counter")

        # ── (C) the markers, as painted ────────────────────────────────────
        before = markers_by_text(tf)
        assert_eq(before[STEP_1], "1.", "C: the first step")
        assert_eq(before[STEP_BOLT], "2.")
        assert_eq(before[STEP_TIGHTEN], "3.")
        assert_eq(
            before[PART_A],
            BULLET,
            "C: a bullet is TEXT here — in Qt it is a painted ellipse with no "
            "accessor at all",
        )
        assert_eq(before[PART_B], BULLET)
        assert_eq(
            before[ROMAN_A], "MMMCMXCIX.", "C: the last value roman can write"
        )

        # ── (D) the CSS range fallback, named ──────────────────────────────
        last = roman["items"][1]
        assert_eq(
            last["marker"],
            "4000.",
            "D: one past the roman range — the number survives, where Qt's "
            "itemText() answers '?' and it does not",
        )
        assert_eq(last["ordinal"], ROMAN_START + 1, "D: the counter is unaffected")
        assert_eq(last["position"], 2, "D: and so is its position")
        assert_eq(roman["style"], "UpperRoman", "D: the DECLARATION is unchanged")
        assert_eq(
            last["rendered_as"],
            "Decimal",
            "D: while the notation that wrote it is named",
        )
        assert last["fell_back"], "D: and the fall itself is reported"
        assert not roman["items"][0]["fell_back"], (
            "D: the item within range did not fall back — the negative control "
            "that keeps `fell_back` from being a constant"
        )

        # ── (E) the marker's painted box is published ──────────────────────
        # Cross-checked against `scene/snapshot`'s rects, which reach the same
        # geometry by an independent path, so this is a check rather than a
        # restatement of one derivation.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        first = steps["items"][0]
        marker_node = find_by_tag(snap, first["marker_tag"])
        assert marker_node is not None, "E: the marker is a painted node"
        assert_eq(
            [first["marker_x"], first["marker_y"]],
            [marker_node["rect"]["x"], marker_node["rect"]["y"]],
            "E: the two surfaces agree about where the marker sits",
        )
        text_node = find_by_tag(snap, first["tag"])
        assert text_node is not None, "E: the item paragraph is painted"
        assert first["marker_x"] + first["marker_width"] <= text_node["rect"]["x"], (
            f"E: the marker sits in the gutter BEFORE its text: "
            f"marker={first} text={text_node['rect']}"
        )
        assert_eq(
            text_node["rect"]["x"] - first["marker_x"],
            STEP_INDENT_PX,
            "E: and the whole distance is what the list declared",
        )
        assert steps["width"] is not None and steps["width"] > 0, (
            "E: the list itself has a box, because a list is an object"
        )

        # ── (F) the same derivation on the paragraph, as scene data ────────
        placement = text_node["list"]
        assert placement is not None, "F: the painted paragraph carries its place"
        assert_eq(set(placement), PLACEMENT_KEYS, "F: its exact key set")
        assert_eq(placement["marker"], first["marker"], "F: one derivation, two surfaces")
        assert_eq(placement["list_tag"], steps["tag"])
        assert_eq(placement["count"], 3)
        assert_eq(
            placement["format"]["style"],
            "Decimal",
            "F: with the DECLARED format beside the numbering it produced",
        )
        assert_eq(placement["format"]["start"], 1)

        # ── (G) negative control: text outside a list is not an item ───────
        for tag, label in ((f"{DOC_TAG}_blk0", "the heading"), (f"{DOC_TAG}_blk1", "the intro")):
            node = find_by_tag(snap, tag)
            assert node is not None, f"G: {label} is painted"
            assert_eq(node["list"], None, f"G: {label} is not a list item")
        item_tags = {i["tag"] for r in lists(tf) for i in r["items"]}
        assert f"{DOC_TAG}_blk0" not in item_tags, "G: and is absent from the census"
        assert_eq(len(item_tags), 7, f"G: seven items and nothing else: {item_tags}")
        chip = find_by_tag(snap, TOGGLE_TAG)
        assert chip is not None, "G: the mode chip is painted"
        label_node = next(c for c in chip["children"] if c.get("type") == "Text")
        assert_eq(
            label_node["list"],
            None,
            "G: an ordinary label is null, so a reader tells an item from any "
            "other text without guessing",
        )

        # ── (H) the WHOLE POINT: one insertion renumbers what follows ──────
        tf.click(path=TOGGLE_TAG)
        wait_until(
            lambda: lists(tf)[0]["count"] == 4,
            timeout=4.0,
            interval=0.03,
            desc="the extra step enters the procedure",
        )
        after = markers_by_text(tf)
        assert_eq(
            after[STEP_1], "1.", "H: the step BEFORE the insert did not move"
        )
        assert_eq(after[STEP_INSERTED], "2.", "H: the new step took its place")
        assert_eq(
            after[STEP_BOLT],
            "3.",
            "H: and every step after it renumbered — nothing the binding wrote "
            "states a number, so this is the derivation and not an author",
        )
        assert_eq(after[STEP_TIGHTEN], "4.")
        assert_eq(after[PART_A], BULLET, "H: the nested list is unaffected")
        assert_eq(after[ROMAN_A], "MMMCMXCIX.", "H: and so is the roman list")
        steps_after = lists(tf)[0]
        assert_eq(steps_after["count"], 4, "H: the list knows its new length")
        assert_eq(
            [i["position"] for i in steps_after["items"]],
            [1, 2, 3, 4],
            "H: positions tile the list with no gap",
        )

        # ── (I) the nested list still restarts and does not interrupt ──────
        parts_after = lists(tf)[1]
        assert_eq(parts_after["count"], 2)
        assert_eq(
            [i["marker"] for i in parts_after["items"]],
            [BULLET, BULLET],
            "I: the inner list is its own sequence",
        )
        assert_eq(
            [i["ordinal"] for i in parts_after["items"]],
            [1, 2],
            "I: counted from its own start, not the outer list's",
        )

        # ── (J) the structure reaches assistive technology ─────────────────
        access = call(tf, "scene/access")
        nodes = {n["tag"]: n for n in access["nodes"]}
        items = [n for n in access["nodes"] if n["role"] == "listitem"]
        assert_eq(len(items), 8, f"J: eight items announced: {len(items)}")
        step_tag = steps_after["items"][0]["tag"]
        step_node = nodes[step_tag]
        assert_eq(step_node["role"], "listitem")
        assert_eq(step_node["position_in_set"], 1, "J: item 1")
        assert_eq(step_node["size_of_set"], 4, "J: of 4")
        assert_eq(step_node["level"], 1, "J: at level 1")
        nested_node = nodes[parts_after["items"][0]["tag"]]
        assert_eq(nested_node["level"], 2, "J: a nested item is heard as nested")
        assert_eq(nested_node["size_of_set"], 2, "J: in its OWN set")
        list_node = nodes[steps_after["tag"]]
        assert_eq(list_node["role"], "list", "J: and the list is a container")
        assert_eq(list_node["size_of_set"], 4)
        assert f"{DOC_TAG}_blk0" not in {n["tag"] for n in items}, (
            "J: the heading is not an item of anything"
        )

        # ── (K) the R1539 census covers this round's wire ──────────────────
        schema = call(tf, "rpc/schema")
        by_name = {t["name"]: t for t in schema["types"]}
        for name in ("TextListWire", "TextListItemWire", "TextListsOutcome"):
            assert name in by_name, f"K: {name} is described by the published census"
        assert_eq(
            {f["name"] for f in by_name["TextListWire"]["shape"]["fields"]},
            LIST_KEYS,
            "K: and the census's key set is the one the live wire answered with",
        )
        assert_eq(
            {f["name"] for f in by_name["TextListItemWire"]["shape"]["fields"]},
            ITEM_KEYS,
            "K: for an item too",
        )
        parent_field = next(
            f for f in by_name["TextListWire"]["shape"]["fields"] if f["name"] == "parent_tag"
        )
        assert_eq(
            parent_field["nullable"],
            True,
            "K: the census states that `parent_tag` may be null, which is the "
            "top-level case an agent must handle",
        )

        # ── (L) the method is discoverable and does not mutate ─────────────
        methods = call(tf, "rpc/methods")["methods"]
        entry = next(m for m in methods if m["name"] == "scene/text_lists")
        assert_eq(entry["occ"], "read", "L: reading a list mutates nothing")
        assert_eq(
            markers_by_text(tf),
            after,
            "L: and the document is where (H) left it",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1559 §5.36 an item is numbered by its place", body))
