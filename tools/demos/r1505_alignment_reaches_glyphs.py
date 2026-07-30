#!/usr/bin/env python3
"""R1505 §5.27 §5.36 §2#7 — the declared alignment reaches the glyphs.

R1504 gave `ColumnLayout` Qt's `QHeaderView::defaultAlignment` and closed
honestly: a label node's rect is its BOX, so the three alignments produce a
byte-identical tree, and it asserted the rule at the surface that OWNS it.
That left the rule proven at one end of a three-link chain:

    surface (`default_alignment`)  →  label NODE  →  GLYPHS

Only the first link had a witness. The middle one had none at all — the scene
carries `style.text_align` on every Text node and has since R55.G.10, and no
assertion anywhere read it, so nothing said the alignment the header chose is
the alignment the label was built with. This demo is that link.

The third link is not a demo's to make: it is pixels, and it lives in
`pinion_shell::headless_screenshot::tests`
::`r1505_declared_alignment_slides_the_ink_within_the_box`, an `#[ignore]`d
headless render that asserts the ink SLIDES across Start / Center / End. The
two halves compose into the whole chain, and this file asserts the premise that
guard models — the label's box is WIDER than the section's text needs, so there
is room to align within. A box with no slack would make the pixel guard's
question meaningless, and that premise lives here because only the running
widget knows its own layout.

Also asserted: the two spellings agree. `TextAlign::as_wire` is the single
table since R1504, so the value the external surface reports and the value the
scene node reports are the same string — a divergence here would mean the
introspection surface and the paint scene disagree about what the header said.

Run:
    cargo build -p hello-column-reorder --release
    python3 tools/demos/r1505_alignment_reaches_glyphs.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
LABEL = "colhdr_label"

HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
BOOT_W = [150, 90, 100, 130, 100]
IDENTITY = list(range(NCOLS))

# The binding's label geometry (examples/hello-column-reorder): a section's
# label box is the cell inset on both sides, less the sort glyph when one is
# shown. Mirrored here so the slack premise is checked against the widget's
# own arithmetic rather than a number this file invented.
CELL_BORDER = 2
LABEL_INSET = 12
GLYPH_W = 24


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _label_node(tf, visual: int) -> dict:
    node = find_by_tag(_paint(tf), f"{LABEL}#{visual}")
    assert node is not None, f"{LABEL}#{visual} is painted"
    return node


def _node_aligns(tf) -> list[str]:
    """What each label node was actually BUILT with, in visual order."""
    return [_label_node(tf, v)["style"]["text_align"] for v in range(NCOLS)]


def _surface_aligns(tf) -> list[str]:
    """What the header SAYS, mapped from logical order into visual order."""
    by_logical = _h(tf, "alignments")
    return [by_logical[lg] for lg in _h(tf, "order")]


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                  desc="the strip paints")

        # ── (A) the rule reaches the node ────────────────────────────
        # R1504 proved the header SAYS Center. This is the first assertion
        # anywhere that the label was BUILT that way.
        assert_eq(_h(tf, "default_alignment"), "Center",
                  "the header's rule, as R1504 left it")                          # 1
        assert_eq(_node_aligns(tf), ["Center"] * NCOLS,
                  "and every label node carries it — the link R1504 left "
                  "with no witness at either end")                                # 2
        assert_eq(_node_aligns(tf), _surface_aligns(tf),
                  "surface and scene agree, in one shared spelling "
                  "(TextAlign::as_wire is the single table since R1504)")         # 3

        # ── (B) the box has slack, which is what makes alignment mean
        # anything. `paint_text` hands the node's own rect.w to the shaper as
        # the width to align within, so a label pinned to its glyphs would
        # render identically under all three rules.
        sizes = _h(tf, "sizes")
        for visual in range(NCOLS):
            node = _label_node(tf, visual)
            logical = _h(tf, "order")[visual]
            sect_w = sizes[logical] - CELL_BORDER
            # No column is sorted at boot, so no section shows the glyph.
            expected = max(sect_w - LABEL_INSET * 2, 1)
            assert_eq(node["rect"]["w"], expected,
                      f"label #{visual} spans its section less the insets, "
                      f"so there is room to align inside it")                     # 4-8
            assert_eq(node["content"], HEADERS[logical],
                      f"and it is column {logical}'s label")                      # 9-13

        # ── (C) changing the rule rebuilds every node ────────────────
        tf.intervene("/external/default_alignment", "Start")
        wait_until(lambda: _node_aligns(tf) == ["Start"] * NCOLS,
                   desc="the nodes follow the rule to Start")
        assert_eq(_node_aligns(tf), _surface_aligns(tf),
                  "still in agreement after the rule moved")                      # 14

        tf.intervene("/external/default_alignment", "End")
        wait_until(lambda: _node_aligns(tf) == ["End"] * NCOLS,
                   desc="…and to End")
        assert_eq(_node_aligns(tf), ["End"] * NCOLS)                              # 15

        tf.intervene("/external/default_alignment", "Center")
        wait_until(lambda: _node_aligns(tf) == ["Center"] * NCOLS,
                   desc="…and back to Qt's default")
        assert_eq(_node_aligns(tf), ["Center"] * NCOLS)                           # 16

        # ── (D) the model's exception reaches exactly one node ───────
        # Qt splits these: the header owns the rule, the model owns the
        # per-section exception. In the tree that split has to show up as one
        # node differing and four not.
        tf.invoke("/external/set_section_alignment", "2:End")
        wait_until(lambda: _h(tf, "section_alignment_override.2") == "End",
                   desc="column 2 takes an exception")
        assert_eq(_node_aligns(tf),
                  ["Center", "Center", "End", "Center", "Center"],
                  "the exception reaches its own label node and no other")        # 17
        assert_eq(_node_aligns(tf), _surface_aligns(tf),
                  "and the surface reports the same per-section picture")         # 18

        # ── (E) the exception is keyed by COLUMN, so it travels ──────
        # This is the assertion the tree can make and the surface cannot on its
        # own: after the move, the node that differs is at a different VISUAL
        # slot, which is what "keyed by column, not by position" means where it
        # is finally observable — in the painted row.
        tf.invoke("/external/move_section", "2:0")
        wait_until(lambda: _h(tf, "order")[0] == 2,
                   desc="Size is dragged to the front")
        assert_eq(_node_aligns(tf),
                  ["End", "Center", "Center", "Center", "Center"],
                  "the End label moved with its column to visual slot 0")         # 19
        assert_eq(_label_node(tf, 0)["content"], "Size",
                  "and it is the same column's label, now painted first")         # 20
        assert_eq(_node_aligns(tf), _surface_aligns(tf))                          # 21

        # ── (F) restore drops the exception, and the nodes show it ───
        saved = _h(tf, "state")
        assert "section_alignments" not in saved, \
            "Qt's saveState carries the rule, never the model's exceptions"       # 22
        tf.intervene("/external/default_alignment", "Start")
        wait_until(lambda: _h(tf, "default_alignment") == "Start",
                   desc="the rule is moved away before restoring")
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "default_alignment") == "Center",
                   desc="the saved rule comes back")
        assert_eq(_node_aligns(tf), ["Center"] * NCOLS,
                  "every node is back on the rule, exception included — "
                  "restore returned what was saved and dropped what was not")     # 23
        assert_eq(_node_aligns(tf), _surface_aligns(tf))                          # 24

        # ── (G) a sorted section still leaves room ───────────────────
        # The sort glyph takes width from the label box. The box must still be
        # wide enough to align within, or the rule would silently stop meaning
        # anything on exactly the column the user is looking at.
        sorted_logical = _h(tf, "order")[0]
        unsorted_w = _label_node(tf, 0)["rect"]["w"]
        tf.invoke("/external/cycle_sort_indicator", sorted_logical)
        wait_until(lambda: _h(tf, "sort_indicator") != "none",
                   desc="the front column sorts")
        node = _label_node(tf, 0)
        sect_w = _h(tf, "sizes")[sorted_logical] - CELL_BORDER
        assert_eq(node["rect"]["w"],
                  max(sect_w - (LABEL_INSET * 2 + GLYPH_W), 1),
                  "the sorted section's label yields the glyph's width")          # 25
        assert 0 < node["rect"]["w"] < unsorted_w, \
            f"and still has a box to align within, a narrower one: " \
            f"{node['rect']['w']} of {unsorted_w}"                                # 26
        assert_eq(_node_aligns(tf)[0], "Center",
                  "the rule is unchanged by sorting")                             # 27

        # ── (H) hidden sections paint no label at all ────────────────
        # The nodes are the only place this is observable: `alignments` answers
        # for every logical column whether or not it is painted, so a rule that
        # reached a hidden section's node would never show up on the surface.
        tf.invoke("/external/set_section_hidden", "1:true")
        wait_until(lambda: _h(tf, "hidden")[1] is True,
                   desc="a column is hidden")
        visible = [v for v in range(NCOLS)
                   if find_by_tag(_paint(tf), f"{LABEL}#{v}") is not None]
        assert_eq(len(visible), NCOLS - 1,
                  "one fewer label node is painted")                              # 28
        for v in visible:
            assert_eq(_label_node(tf, v)["style"]["text_align"], "Center",
                      f"and every painted label #{v} still carries the rule")     # 29-32
        assert_eq(len(_h(tf, "alignments")), NCOLS,
                  "while the surface still answers for all five columns, "
                  "painted or not")                                               # 33

        tf.invoke("/external/set_section_hidden", "1:false")
        wait_until(lambda: _h(tf, "hidden")[1] is False,
                   desc="and it comes back")
        assert_eq(_node_aligns(tf), ["Center"] * NCOLS)                           # 34
        assert_eq(_node_aligns(tf), _surface_aligns(tf))                          # 35


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1505 §5.27 §5.36 §2#7 — the declared alignment reaches the glyphs: "
        "surface → label node (here) → pixels (headless guard)",
        body,
    ))
