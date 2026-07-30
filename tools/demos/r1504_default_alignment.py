#!/usr/bin/env python3
"""R1504 §5.27 §2#7 §2#2 — the header says where its labels sit.

Qt reference: `QHeaderView::defaultAlignment`, which `QHeaderViewPrivate::write()`
serialises, and whose horizontal-header default is `Qt::AlignCenter`
(`QHeaderViewPrivate::setDefaultValues`). Qt keeps the per-section exception
somewhere else entirely — the MODEL answers
`headerData(section, orientation, Qt::TextAlignmentRole)` — and `saveState()`
does not carry it. This round mirrors that split: the header's rule is state,
the model's exceptions are not.

Measured on this very binding before the round, over the real wire:

    query  alignment / default_alignment / alignments / section_alignment.0
           -> UnknownIntrospectPath  (all four)
    state  -> 14 keys, none of them about alignment
    paint  -> every label pinned 12px from its section's left edge

So the header painted flush left while Qt's default is centred, and there was
no way to ask for either — the default was wrong AND unaskable.

WHAT THE PAINT TREE CAN AND CANNOT SHOW, measured in this run's own probe:
a label's node rect is its BOX, not its glyph extent, so the three alignments
produce byte-identical trees. That is not a defect to route around — alignment
places glyphs inside a box at paint time, and §2#7 keeps pixels out of
introspection. The rule is therefore asserted on the surface that OWNS it
(`default_alignment` / `alignments` / `section_alignment.<logical>`), exactly
as a Qt client asks `defaultAlignment()` rather than measuring the widget. What
the tree does prove is the box, and the box is what this round changed.

What this asserts:

  (A) THE RULE IS READABLE, AND IT IS QT'S — centred, where the binding used to
      paint flush left. All four paths that answered `UnknownIntrospectPath`
      before the round now answer.
  (B) THE BOX SPANS THE SECTION — the label used to be a bare extent pinned
      12px in; it is now a box inset 12px at each end. Measured against the
      section rect, and the reason R1499's `pointer_transparent` becomes
      load-bearing in EVERY section rather than the two whose strings happened
      to be wide enough.
  (C) THE RULE IS WRITABLE, AND A SPELLING IT DOES NOT KNOW IS REFUSED — not
      silently defaulted, which is what the lenient `TextField` decoder does and
      what this channel deliberately does not.
  (D) THE MODEL'S EXCEPTION IS INDEPENDENT OF THE HEADER'S RULE — one section
      moves, the others do not, and `default` is the spelling that hands it
      back. Without that spelling an exception could be set and never cleared.
  (E) THE EXCEPTION TRAVELS WITH ITS COLUMN — keyed by logical section, so
      dragging the column carries its alignment, the same rule the sizes and
      the sort indicator follow.
  (F) SAVED STATE CARRIES THE RULE AND NOT THE EXCEPTIONS — Qt's split, so a
      restore hands back a header whose sections all defer.
  (G) AN OLDER SNAPSHOT DECODES AS `Start` — what the pre-R1504 header actually
      painted, NOT the construction default. The same new-vs-old split R1496
      drew for `sections_movable`.
  (H) THE SURFACE DECLARES ALL OF IT — R1501's gate, extended: the five new
      paths are in `$schema`, and `set_section_alignment` declares itself an
      invoke channel rather than being told apart by the shape of its name.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1504_default_alignment.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
LAYOUT_TAG = "colreorder_layout"

HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
BOOT_W = [150, 90, 100, 130, 100]
IDENTITY = list(range(NCOLS))

FLOOR = 40
UNBOUNDED = 2**32 - 1
DEFAULT_SIZE = 100
LABEL_INSET = 12
GLYPH_W = 24


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    assert node is not None, f"{tag} is painted"
    return node["rect"]


def _state_without_alignment(tf) -> dict:
    """The boot layout as an OLDER snapshot: every field but this round's."""
    return {
        "order": IDENTITY,
        "sizes": BOOT_W,
        "hidden": [False] * NCOLS,
        "modes": ["interactive"] * NCOLS,
        "sort_indicator": "none",
        "sort_indicator_shown": True,
        "default_section_size": DEFAULT_SIZE,
        "min_section_size": FLOOR,
        "max_section_size": UNBOUNDED,
        "cascading_section_resizes": False,
        "stretch_last_section": False,
    }


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) the rule is readable, and it is Qt's ──────────────────
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                   desc="the strip paints")
        assert_eq(_h(tf, "default_alignment"), "Center",
                  "Qt centres a horizontal header")                              # 1
        assert_eq(_h(tf, "alignments"), ["Center"] * NCOLS,
                  "and with no exceptions every section follows it")              # 2
        assert_eq(_h(tf, "section_alignment.0"), "Center")                        # 3
        assert_eq(_h(tf, "section_alignment_override.0"), None,
                  "the override is the one that can answer nothing")              # 4
        assert "| align C" in _readout(tf), \
            f"the readout names the rule: {_readout(tf)}"                         # 5

        # ── (B) the box spans the section ─────────────────────────────
        for visual in range(3):
            sec = _rect(tf, f"{HDR}#{visual}")
            lab = _rect(tf, f"{HDR}_label#{visual}")
            # The arrow's room is reserved only where an arrow is painted, which
            # is the sorted column alone — the first draft of this assertion
            # reserved it everywhere and the demo refused it (124 vs 100).
            arrow = find_by_tag(_paint(tf), f"{HDR}_sort#{visual}") is not None
            reserved = LABEL_INSET * 2 + (GLYPH_W if arrow else 0)
            assert_eq(lab["x"], sec["x"] + LABEL_INSET,
                      f"section {visual}: the box starts one inset in")           # 6,9,12
            assert_eq(lab["w"], sec["w"] - reserved,
                      f"section {visual}: and reserves the arrow's room "
                      f"only where there is one (arrow={arrow})")                 # 7,10,13
            # R1499's hazard, now uniform: `scene/click` presses a rect CENTRE,
            # and the label covers that point in every section rather than in
            # the two whose strings happened to be wide enough (R1497).
            lab_mid = lab["x"] + lab["w"] / 2.0
            sec_mid = sec["x"] + sec["w"] / 2.0
            assert abs(lab_mid - sec_mid) <= 1.0, \
                f"section {visual}: the box is centred on the section " \
                f"({lab_mid} vs {sec_mid})"                                       # 8,11,14

        # ── (C) the rule is writable, and a bad spelling is refused ───
        tf.intervene("/external/default_alignment", "Start")
        wait_until(lambda: _h(tf, "default_alignment") == "Start",
                   desc="the rule moves to Start")
        assert_eq(_h(tf, "alignments"), ["Start"] * NCOLS,
                  "every section follows the new rule")                           # 15
        assert "| align S" in _readout(tf), \
            f"and the readout follows it: {_readout(tf)}"                         # 16
        # Typed, not merely failed: the refusal travels the real wire with the
        # reason, so "refused" cannot be a transport hiccup wearing a green tick.
        assert_rpc_error(
            lambda: tf.intervene("/external/default_alignment", "middle"),
            data="InterveneTypeMismatch",
        )                                                                         # 17
        assert_eq(_h(tf, "default_alignment"), "Start",
                  "and the refusal changed nothing")                              # 18

        # ── (D) the model's exception is independent ──────────────────
        tf.invoke("/external/set_section_alignment", "2:End")
        wait_until(lambda: _h(tf, "section_alignment_override.2") == "End",
                   desc="Size takes an exception")
        assert_eq(_h(tf, "section_alignment.2"), "End",
                  "the section paints with its own")                              # 19
        assert_eq(_h(tf, "alignments"),
                  ["Start", "Start", "End", "Start", "Start"],
                  "and only that one moved")                                      # 20
        assert_eq(_h(tf, "section_alignment_override.0"), None,
                  "its neighbours still defer")                                   # 21
        assert f"except {HEADERS[2]}=E" in _readout(tf), \
            f"the readout names the exception: {_readout(tf)}"                    # 22
        assert_rpc_error(
            lambda: tf.invoke("/external/set_section_alignment", "2:middle"),
            data="InvokeRejected",
        )                                                                         # 23
        assert_rpc_error(
            lambda: tf.invoke("/external/set_section_alignment", "9:End"),
            data="InvokeRejected",
        )                                                                         # 24

        # The rule still moves independently of the exception.
        tf.intervene("/external/default_alignment", "Center")
        wait_until(lambda: _h(tf, "default_alignment") == "Center",
                   desc="the rule moves under the exception")
        assert_eq(_h(tf, "alignments"),
                  ["Center", "Center", "End", "Center", "Center"],
                  "the excepted section did not follow it")                       # 25

        # ── (E) the exception travels with its column ─────────────────
        tf.invoke("/external/move_section", "2:0")
        wait_until(lambda: _h(tf, "order")[0] == 2, desc="Size dragged to the front")
        assert_eq(_h(tf, "section_alignment.2"), "End",
                  "the exception is keyed by column, so it went along")           # 26
        assert_eq(_h(tf, "alignments"),
                  ["Center", "Center", "End", "Center", "Center"],
                  "the row is keyed logically and did not change shape")          # 27

        # ── (F) saved state carries the rule, not the exceptions ──────
        saved = _h(tf, "state")
        assert_eq(saved["default_alignment"], "Center",
                  "the header's rule is in the snapshot")                         # 28
        assert "section_alignments" not in saved, \
            "and the model's exceptions are not, as in Qt"                        # 29

        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "order")[0] == 2, desc="the state restores")
        assert_eq(_h(tf, "default_alignment"), "Center",
                  "the rule came back")                                           # 30
        assert_eq(_h(tf, "section_alignment_override.2"), None,
                  "and the exception did not, because the state never named it")  # 31
        assert_eq(_h(tf, "alignments"), ["Center"] * NCOLS,
                  "so every section defers to the rule")                          # 32

        # ── (G) an older snapshot decodes as Start ────────────────────
        tf.intervene("/external/state", _state_without_alignment(tf))
        wait_until(lambda: _h(tf, "order") == IDENTITY, desc="the older shape restores")
        assert_eq(_h(tf, "default_alignment"), "Start",
                  "absent describes the header that painted flush left")          # 33
        assert_eq(_h(tf, "alignments"), ["Start"] * NCOLS)                        # 34
        assert "| align S" in _readout(tf), \
            f"and the readout says so: {_readout(tf)}"                            # 35

        # ── (H) the surface declares all of it ────────────────────────
        schema = tf.query("/external/$schema")
        paths = {f["path"] for f in schema}
        for p in ("default_alignment", "alignments", "section_alignment.<logical>",
                  "section_alignment_override.<logical>", "set_section_alignment"):
            assert p in paths, f"{p!r} is declared: {sorted(paths)}"              # 36-40
        by_path = {f["path"]: f for f in schema}
        assert_eq(by_path["set_section_alignment"].get("channel"), "invoke",
                  "the action declares itself one rather than being guessed "
                  "from the shape of its name")                                   # 41
        assert_eq(by_path["default_alignment"].get("channel"), None,
                  "and a readable path carries no channel key at all")            # 42
        assert_eq(
            [a["domain"] for a in by_path["section_alignment.<logical>"]["args"]],
            [{"kind": "index_of", "count_path": "count"}],
            "the family names the bound its argument indexes, in the typed "
            "shape R1353 gave a domain rather than the bare path it used to be",
        )                                                                         # 43


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1504 §5.27 §2#7 — QHeaderView defaultAlignment: the header says "
        "where its labels sit",
        body,
    ))
