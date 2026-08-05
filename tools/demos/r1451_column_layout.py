#!/usr/bin/env python3
"""R1451 §5.27 §5.40 §5.51 §2#7 §2#2 — a column carries its width where it goes.

Qt reference: `QHeaderView` section state. Qt keys `sectionSize()` and
`isSectionHidden()` by the **logical** section, which is why a Qt column you
resize stays that wide wherever you drag it. pinion had all the pieces and none
of the composition: widths (R785) were indexed by SCREEN POSITION, visibility
(R990) lived in a different binding, and order (R1450) knew about neither. So
"move this column" left its width behind, and there was no way to say otherwise.

R1451 gives that state one home (`ColumnLayout`) keyed as Qt keys it, and adds
`swapSections`, `logicalIndexAt`, `sectionPosition`, and the whole
`saveState`/`restoreState` round-trip.

And where Qt cannot follow: `QHeaderView::saveState()` is an opaque versioned
`QByteArray`. An agent cannot read "which column is third and how wide is it"
out of it, nor author one without a live widget. Here every field is typed data
in both directions, and the painted geometry itself is published.

What this asserts:

  (A) BOOT — five sections at DELIBERATELY NON-UNIFORM widths, and the painted
      rects agree with the model. A uniform strip could not tell "the width
      travelled with the column" apart from "the widths never moved".
  (B) THE CLAIM — resize a section, then move it, and its width arrives with it.
      Checked in PIXELS (the painted header rect) and against the BODY beneath,
      not only against the model's own readout.
  (C) HIDING KEEPS THE SECTION — a hidden column leaves the paint, the labels,
      and the AT tree, while keeping its visual index and its size, and comes
      back where it was.
  (D) logicalIndexAt — a hit test over non-uniform widths that steps over the
      hidden section. A uniform `x / col_width` gets every one of these wrong.
  (E) saveState / restoreState — all three axes round-trip through typed data.
  (F) TYPED REFUSALS — a wrong shape and an impossible value are different
      errors, and neither applies half of itself.
  (G) swapSections IS NOT moveSection — the same two indices produce different
      orders, which is why Qt has both.
  (H) §2 #2 — the keyboard (`]` resize, `h` hide) reaches the same model as the
      wire, and lands on the LOGICAL section rather than the screen slot.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1451_column_layout.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    assert_out_of_range,
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
# Per-LOGICAL-section boot widths, non-uniform on purpose.
BOOT_W = [150, 90, 100, 130, 100]
# Row 0 of the source model, by logical column.
ROW0 = ["report.pdf", "PDF", "2.1 MB", "2026-06-01", "coin"]
# The binding paints each section `size - GUTTER` wide so neighbours have a seam.
GUTTER = 2
# `colbody#<row>_<visual>` tags the cell's TEXT, which the binding insets inside
# its column box; the header cell tag is the box itself.
LABEL_INSET = 12
# The `[` / `]` keyboard step.
STEP = 20

# R1452 added the sizing policy to the saved state (Qt's saveState carries it
# too), so the boot snapshot is this shape now — the assertions below still
# check "saveState reads the WHOLE thing", which is why they moved with it.
IDENTITY = {
    "order": [0, 1, 2, 3, 4],
    "sizes": BOOT_W,
    "hidden": [False] * NCOLS,
    "modes": ["interactive"] * NCOLS,
    # R1491 — the snapshot grew the sort indicator, which Qt's `saveState()`
    # has always carried. This assertion is a WHOLE-object equality on purpose:
    # a field added to the peer of `saveState` must be visible here rather than
    # slipping in unremarked, which is what a field-by-field check would allow.
    "sort_indicator": "none",
    "sort_indicator_shown": True,
    # R1493 — and again, for the three scalar rules `saveState()` carries: the
    # default section size and both bounds. This demo caught their arrival on
    # the round that added them, which is exactly what the whole-object form is
    # for; the round then had to say so here rather than quietly widen a
    # per-field check.
    "default_section_size": 100,
    "min_section_size": 40,
    "max_section_size": 2**32 - 1,
    # R1494 — and again for Qt's `cascadingSectionResizes`, which `saveState()`
    # carries. Three rounds running, this assertion has been the thing that
    # made a field's arrival visible instead of letting it in unremarked; the
    # cost of updating it is the price of that, and it is cheap.
    "cascading_section_resizes": False,
    # R1496 — a fourth time, and the largest addition yet: Qt's two interaction
    # permissions (`movableSections` / `clickableSections`) and the sampling
    # bound (`resizeContentsPrecision`), all three of which
    # `QHeaderViewPrivate::write()` serialises. The bound is the interesting
    # one: R1454 put it on the header and DECIDED to keep it out of the
    # snapshot, so this equality had been agreeing with a stated decision rather
    # than with Qt. `True` for both permissions is this app's declaration, not
    # `ColumnLayout`'s default, which is Qt's `false`.
    "sections_movable": True,
    "sections_clickable": True,
    "resize_contents_precision": 1000,
    # R1498 — a fifth time, for Qt's `stretchLastSection`. `False` is both Qt's
    # default and this app's posture: the strip is 640 wide and the sections sum
    # to 570, and until a consumer asks for the rule those 70 pixels stay
    # unpainted. Five rounds running now, this equality has been the thing that
    # makes a field's arrival visible.
    "stretch_last_section": False,
    # R1504 — a SIXTH time, for Qt's `defaultAlignment`. `"Center"` is Qt's
    # horizontal-header default and what `ColumnLayout` constructs with; note it
    # is NOT what an absent field decodes to (`"Start"`, what the pre-R1504
    # header actually painted), so this fixture and the older-snapshot decode
    # deliberately disagree — the R1496 split, one round later.
    #
    # What is NOT here is the per-section exception. Qt keeps it in the model
    # (`headerData(TextAlignmentRole)`) and its `saveState()` does not carry it,
    # so this equality asserts an absence as much as a presence: a round that
    # started saving the exceptions would fail here, which is the point.
    "default_alignment": "Center",
    # R1510 — a SEVENTH time, for Qt's `highlightSections` (serialised as
    # `highlightSelected`). `False` is Qt's default AND what an absent field
    # decodes to, so unlike `default_alignment` above this fixture and the
    # older-snapshot decode agree — the pre-R1510 header had no selection input
    # at all, so "did not highlight" describes both.
    #
    # What is NOT here is the SELECTION. Qt's header reads the view's selection
    # model and `saveState()` cannot reach it, so this equality asserts that
    # absence too: a round that started saving the published coverage would fail
    # here. Seven rounds running, this whole-object form has been the thing that
    # makes a field's arrival visible instead of letting it in unremarked.
    "highlight_sections": False,
}


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    return None if node is None else node["rect"]


def _placements(tf):
    return _h(tf, "placements")


def _access(tf):
    return tf.request("scene/access").result["nodes"]


def _colheader_names(tf) -> list[str]:
    return [n.get("name") for n in _access(tf) if n.get("role") == "columnheader"]


def _reset(tf) -> None:
    """Back to the boot layout, through the restore path itself."""
    tf.intervene("/external/state", IDENTITY)
    wait_until(lambda: _h(tf, "state") == IDENTITY, desc="layout reset")


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) boot: non-uniform sections, and the pixels agree ──────
        wait_until(lambda: _rect(tf, f"{HDR}#0") is not None, desc="the strip paints")
        assert_eq(_h(tf, "count"), NCOLS, "boot: five sections")                        # 1
        assert_eq(_h(tf, "order"), [0, 1, 2, 3, 4], "boot: identity order")             # 2
        assert_eq(_h(tf, "sizes"), BOOT_W, "boot: the sizes are non-uniform")           # 3
        assert_eq(_h(tf, "hidden"), [False] * NCOLS, "boot: nothing hidden")            # 4
        assert_eq(_h(tf, "visible_total"), sum(BOOT_W), "boot: the strip's width")      # 5
        assert_eq(_h(tf, "state"), IDENTITY, "boot: saveState reads the whole thing")   # 6

        places = _placements(tf)
        assert_eq(len(places), NCOLS, "every section is painted")                       # 7
        assert_eq([p["x"] for p in places], [0, 150, 240, 340, 470],
                  "the offsets accumulate the individual widths")                       # 8
        assert_eq([p["logical"] for p in places], [0, 1, 2, 3, 4], "in schema order")   # 9
        # The published geometry is the painted geometry, not a parallel claim.
        for p in places:
            assert_eq(_rect(tf, f"{HDR}#{p['visual']}")["w"], p["size"] - GUTTER,
                      f"section {p['visual']} paints at its own width")                 # 10-14
            assert_eq(_rect(tf, f"colbody#0_{p['visual']}")["x"],
                      _rect(tf, f"{HDR}#{p['visual']}")["x"] + LABEL_INSET,
                      f"body column {p['visual']} sits under its header")               # 15-19
        assert "Name=150" in find_by_tag(_paint(tf), LAYOUT_TAG)["content"], \
            "the sizes are scene-as-data, keyed by column NAME not by slot"             # 20

        # ── (B) THE CLAIM: a resized column keeps its width when it moves ─
        assert_eq(tf.invoke("/external/resize_section", "0:240"), 240,
                  "resizeSection reports the applied size")                             # 21
        wait_until(lambda: _h(tf, "sizes")[0] == 240, desc="Name is 240 wide")
        tf.invoke("/external/move_section", "0:2")
        wait_until(lambda: _h(tf, "order") == [1, 2, 0, 3, 4], desc="Name moves third")

        places = _placements(tf)
        name = places[2]
        assert_eq(name["logical"], 0, "the third section is Name")                      # 22
        assert_eq(name["size"], 240, "AND IT KEPT ITS WIDTH")                           # 23
        assert_eq(name["x"], 190, "Type (90) + Size (100) precede it")                  # 24
        # A position-keyed width model paints 150 here — it never learned the
        # column moved. This is the pixel that is the round.
        assert_eq(_rect(tf, f"{HDR}#2")["w"], 240 - GUTTER,
                  "and the PAINTED header is 240 wide, not 150")                        # 25
        assert_eq(_rect(tf, "colbody#0_2")["x"],
                  _rect(tf, f"{HDR}#2")["x"] + LABEL_INSET,
                  "the body column travelled to the same place")                        # 26
        assert_eq(find_by_tag(_paint(tf), "colbody#0_2")["content"], ROW0[0],
                  "and it is still Name's data underneath")                             # 27
        assert_eq(_h(tf, "section_position.0"), 190, "sectionPosition agrees")          # 28
        assert_eq(_h(tf, "visible_total"), sum(BOOT_W) + 90, "the strip grew by 90")    # 29

        # ── (C) hiding keeps the section, it only stops painting it ───
        assert_eq(tf.invoke("/external/set_section_hidden", "1:true"), [2, 0, 3, 4],
                  "setSectionHidden reports what is now painted")                       # 30
        wait_until(lambda: _h(tf, "hidden_count") == 1, desc="Type hides")
        assert_eq(_h(tf, "labels"), ["Size", "Name", "Modified", "Owner"],
                  "the labels lose it")                                                 # 31
        assert_eq(_colheader_names(tf), ["Size", "Name", "Modified", "Owner"],
                  "and so does the AT tree, from the same projection")                  # 32
        assert_eq(_rect(tf, f"{HDR}#0"), None, "the hidden section paints nothing")     # 33
        assert_eq([p["visual"] for p in _placements(tf)], [1, 2, 3, 4],
                  "the survivors keep the visual indices the permutation knows")        # 34
        assert_eq(_placements(tf)[0]["x"], 0, "and the strip closes the gap")           # 35
        assert_eq(_h(tf, "visual_index.1"), 0, "Type still holds its place")            # 36
        assert_eq(_h(tf, "section_size.1"), 90, "and its size")                         # 37
        assert_eq(_h(tf, "section_position.1"), None, "but it is painted nowhere")      # 38
        assert_eq(_h(tf, "visible_total"), sum(BOOT_W), "the strip lost exactly 90")    # 39

        # ── (D) logicalIndexAt over non-uniform widths ────────────────
        # Painted now: Size(100) Name(240) Modified(130) Owner(100).
        assert_eq(_h(tf, "logical_index_at.0"), 2, "x=0 is Size")                       # 40
        assert_eq(_h(tf, "logical_index_at.99"), 2, "and so is x=99")                   # 41
        assert_eq(_h(tf, "logical_index_at.100"), 0,
                  "x=100 is Name — a uniform hit test says otherwise")                  # 42
        assert_eq(_h(tf, "logical_index_at.339"), 0, "Name runs to 339")                # 43
        assert_eq(_h(tf, "logical_index_at.340"), 3, "then Modified")                   # 44
        assert_eq(_h(tf, "logical_index_at.9999"), None, "past the last section")       # 45

        # ── (C') showing it puts it back where it was ─────────────────
        tf.invoke("/external/set_section_hidden", "1:false")
        wait_until(lambda: _h(tf, "hidden_count") == 0, desc="Type comes back")
        assert_eq(_h(tf, "labels"), ["Type", "Size", "Name", "Modified", "Owner"],
                  "back in its own place, not appended")                                # 46
        assert_eq(_h(tf, "section_size.1"), 90, "and at the size it kept")              # 47

        # ── (E) saveState / restoreState, every axis at once ──────────
        tf.invoke("/external/set_section_hidden", "3:true")
        tf.invoke("/external/resize_section", "4:200")
        wait_until(lambda: _h(tf, "sizes")[4] == 200, desc="a distinctive layout")
        saved = _h(tf, "state")
        assert_eq(saved["order"], [1, 2, 0, 3, 4], "the snapshot carries the order")    # 48
        assert_eq(saved["sizes"], [240, 90, 100, 130, 200], "the LOGICAL sizes")        # 49
        assert_eq(saved["hidden"], [False, False, False, True, False], "the flags")     # 50

        tf.invoke("/external/swap_sections", "0:4")
        tf.invoke("/external/resize_section", "4:60")
        tf.invoke("/external/set_section_hidden", "3:false")
        wait_until(lambda: _h(tf, "state") != saved, desc="the layout drifts away")
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "state") == saved, desc="restoreState brings it back")
        assert_eq(_h(tf, "labels"), ["Type", "Size", "Name", "Owner"],
                  "the restored layout is what is painted")                             # 51
        assert_eq(_placements(tf)[2]["size"], 240, "sizes restored onto their sections") # 52

        # ── (F) refusals are typed, and refuse WHOLE ──────────────────
        assert_out_of_range(lambda: tf.intervene("/external/state", {
            "order": [0, 0, 2, 3, 4], "sizes": [9] * 5, "hidden": [True] * 5,
        }), saying="an id repeats or is out of range")                                      # 53
        assert_out_of_range(lambda: tf.intervene("/external/state", {
            "order": [0, 1, 2, 3, 4], "sizes": [9, 9], "hidden": [False] * 5,
        }), saying="needs 5 entries, not 2")                                            # 54
        assert_rpc_error(lambda: tf.intervene("/external/state", 7),
                         data="InterveneTypeMismatch")                                  # 55
        assert_rpc_error(lambda: tf.intervene("/external/hidden", [1, 2, 3, 4, 5]),
                         data="InterveneTypeMismatch")                                  # 56
        assert_eq(_h(tf, "state"), saved,
                  "four refusals, and not one field of them landed")                    # 57
        assert_action_refused(
            lambda: tf.invoke("/external/resize_section", "9:100"),
            saying="no section 9 in this header",
        )                                                                               # 58
        assert_action_refused(
            lambda: tf.invoke("/external/set_section_hidden", "0:maybe"),
            saying='malformed argument "0:maybe"',
        )                                                                               # 59

        # ── (G) swapSections is not moveSection ───────────────────────
        _reset(tf)
        assert_eq(tf.invoke("/external/swap_sections", "0:4"), [4, 1, 2, 3, 0],
                  "a swap exchanges two sections and displaces nothing else")           # 60
        _reset(tf)
        assert_eq(tf.invoke("/external/move_section", "0:4"), [1, 2, 3, 4, 0],
                  "a move shifts every section it passes — a different answer")         # 61

        # ── (H) §2 #2 — the keyboard reaches the same model ───────────
        _reset(tf)
        tf.request("focus/set", {"tag": HDR})
        assert_eq(tf.request("focus/get").result.get("focused"), HDR, "the strip focuses")  # 62
        tf.key(path=f"{HDR}#0", name="ArrowRight")
        wait_until(lambda: _h(tf, "focused_index") == 0, desc="cursor on section 0")
        tf.key(path=f"{HDR}#0", name="]")
        wait_until(lambda: _h(tf, "sizes")[0] == BOOT_W[0] + STEP,
                   desc="the keyboard widened Name")                                    # 63
        assert_eq(_rect(tf, f"{HDR}#0")["w"], BOOT_W[0] + STEP - GUTTER,
                  "and the pixels followed the key")                                    # 64
        tf.key(path=f"{HDR}#0", name="[")
        wait_until(lambda: _h(tf, "sizes")[0] == BOOT_W[0], desc="and narrowed it back")

        # The key aims at a SLOT; resize_section names a SECTION. Move a column
        # and the two must still mean the same thing.
        tf.invoke("/external/move_section", "0:4")
        wait_until(lambda: _h(tf, "order") == [1, 2, 3, 4, 0], desc="Name to the end")
        tf.key(path=f"{HDR}#4", name="End")
        wait_until(lambda: _h(tf, "focused_index") == 4, desc="cursor to the last slot")
        tf.key(path=f"{HDR}#4", name="]")
        wait_until(lambda: _h(tf, "sizes")[0] == BOOT_W[0] + STEP,
                   desc="the LOGICAL section under the cursor grew")                    # 65
        assert_eq(_h(tf, "sizes")[4], BOOT_W[4],
                  "and Owner, which used to be last, did not")                          # 66

        # `h` hides through the same call the wire makes, and the cursor steps
        # off the section it just hid.
        tf.key(path=f"{HDR}#4", name="h")
        wait_until(lambda: _h(tf, "hidden")[0] is True, desc="the keyboard hid Name")   # 67
        assert_eq(_h(tf, "labels"), ["Type", "Size", "Modified", "Owner"],
                  "and the strip lost it")                                              # 68
        assert _h(tf, "focused_index") != 4, "the cursor left the unpainted section"    # 69
        assert_eq(_h(tf, "section_size.0"), BOOT_W[0] + STEP,
                  "a hidden section still remembers the width the key gave it")         # 70


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1451 §5.27 §2#7 §2#2 — QHeaderView section state: size, order, hidden",
        body,
    ))
