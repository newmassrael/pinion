#!/usr/bin/env python3
"""R1510 §5.27 §2#2 §2#7 — the header dresses the sections the selection reaches.

the toolkit reference: `highlightSections`, whose default is `false` and which
`write()` serialises as `highlightSelected`. The toolkit resolves a
selection into two INDEPENDENT style flags in `paintSection` — `State_On` when
`sectionIntersectsSelection(logical)`, `State_Sunken` when
`isSectionSelected(logical)` (the whole section) — and gates both on the rule.

The division this round mirrors: THE HEADER OWNS THE RULE, THE SELECTION IS NOT
THE HEADER'S. The toolkit's header holds a pointer to the view's selection model and only
ever reads it, so `saveState()` carries the permission to highlight and never the
thing highlighted. `ColumnLayout` has neither a selection model nor a row count,
so the consumer that owns the rows publishes the coverage — the peer of the
content widths the toolkit's header gets from `sectionSizeFromContents()`.

Measured on this very binding before the round, over the real wire:

    query  highlight_sections / highlight_selected / selected_sections /
           section_selected.0 / selection / selected / highlight_sections_shown
           -> UnknownIntrospectPath  (all seven)
    state  -> 15 keys, none of them about a selection or a highlight
    $schema-> 60 fields, no `highlight` and no `select` among them
    paint  -> all five labels at font_weight 400, all five cells one fill

So a selection had no way to reach the header, and no way to be asked about.

WHAT THE PAINT TREE CAN SHOW HERE, and why that differs from R1504: alignment
places glyphs inside a box at paint time, so R1504's three alignments produced a
byte-identical tree and the rule could only be asserted on the surface that owns
it. Both of this round's channels are node PROPERTIES — `font_weight` and
`fg_color`, both in the snapshot — so the tree is a real witness this time, and
the pixel half is `pinion-shell`'s
`r1510_full_coverage_accents_the_label_the_other_levels_do_not`.

What this asserts:

  (A) THE RULE IS READABLE, AND IT IS the toolkit'S — `false`, where before the round
      there was no rule at all. Seven paths that answered `UnknownIntrospectPath`
      now answer or are declared.
  (B) THE SELECTION IS AN INPUT AND THE RULE IS A GATE — `selections` is what the
      consumer published and never moves when the rule does; `highlights` is what
      the header paints and is entirely the rule's to suppress. A client reading
      both can tell an unselected section from an un-permitted one, which is the
      §2 #7 distinction one row alone erases.
  (C) THE PAINT FOLLOWS THE EFFECTIVE ROW — weight for the toolkit's `State_On`, label
      colour for `State_Sunken`, and the section FILL untouched by either, so a
      highlighted section still shows the keyboard cursor.
  (D) THE COVERAGE TRAVELS WITH ITS COLUMN — keyed by logical section, the rule
      the sizes, the modes and the alignments all follow, so dragging a column
      carries its highlight.
  (E) SAVED STATE CARRIES THE RULE AND NOT THE SELECTION — the toolkit's split. And the
      selection SURVIVES a restore, which is the opposite of what happens to
      R1504's alignment exceptions: those are header data, this belongs to the
      view's selection model, which a header restore cannot reach.
  (F) AN OLDER SNAPSHOT DOES NOT HIGHLIGHT — and here the toolkit's default and the old
      header agree, unlike `sections_movable` (R1496) and `default_alignment`
      (R1504).
  (G) THE GESTURE PATH REACHES BOTH — `H` toggles the rule, `s` cycles the
      cursor's column. The keyboard is this binding's stand-in for the cell
      picking a real grid does.
  (H) REFUSALS ARE TYPED — an unknown spelling, a section that does not exist,
      and a row of the wrong length are each refused with a reason rather than
      silently defaulted, in the vocabulary this surface already speaks: a
      wrong-length row is `OutOfRange` and an unknown spelling is
      `InterveneTypeMismatch`, which is exactly what the `content_widths` peer
      answers (measured, both slots, before this was asserted).
  (I) THE SURFACE DECLARES ALL OF IT — R1501's gate: every new path is in
      `$schema`, and the action declares itself an invoke channel.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock sleeps.

Run from the workspace root:
    python3 tools/demos/r1510_header_highlight.py
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
IDENTITY = list(range(NCOLS))

NONE_ROW = ["none"] * NCOLS
WEIGHT_NORMAL = 400
WEIGHT_BOLD = 700


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _label_style(tf, visual: int) -> dict:
    node = find_by_tag(_paint(tf), f"{HDR}_label#{visual}")
    assert node is not None, f"{HDR}_label#{visual} is painted"
    return node["style"]


def _cell_fill(tf, visual: int):
    node = find_by_tag(_paint(tf), f"{HDR}#{visual}")
    assert node is not None, f"{HDR}#{visual} is painted"
    return node["style"]["fill"]


def _state_without_highlight(tf) -> dict:
    """The current state as an OLDER snapshot: every field but this round's."""
    older = dict(_h(tf, "state"))
    older.pop("highlight_sections", None)
    return older


def _cycle_selection(tf, logical: int) -> None:
    """Drive the binding's own `s` gesture on a LOGICAL column, cursor and all.

    The visual index is resolved over the wire rather than assumed: `s` cycles
    whichever column the cursor is on, and by this point in the demo the
    permutation has been moved, so a hardcoded visual would cycle a different
    column than the one being asserted about.
    """
    visual = _h(tf, f"visual_index.{logical}")
    assert visual is not None, f"logical {logical} is painted somewhere"
    tf.key(path=f"{HDR}#{visual}", name="Home")
    for _ in range(visual):
        tf.key(path=f"{HDR}#{visual}", name="ArrowRight")
    wait_until(lambda: _h(tf, "focused_index") == visual,
               desc=f"the cursor reaches visual {visual}")
    tf.key(path=f"{HDR}#{visual}", name="s")


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        _main(tf)


def _main(tf: RpcSubprocess) -> None:
    wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
               desc="the strip paints")
    # ── (A) the rule is readable, and it is the toolkit's ───────────────────
    assert_eq(_h(tf, "highlight_sections"), False,
              "the toolkit's default, and what a header with no selection input did")   # 1
    assert_eq(_h(tf, "selections"), NONE_ROW,
              "nothing has been published, so nothing is selected")            # 2
    assert_eq(_h(tf, "highlights"), NONE_ROW,
              "and nothing is dressed")                                        # 3
    assert_eq(_h(tf, "section_selection.0"), "none")                           # 4
    assert_eq(_h(tf, "section_highlight.0"), "none")                           # 5
    assert "| highlight off" in _readout(tf), \
        f"the readout names the rule: {_readout(tf)}"                          # 6

    # ── (B) the selection is an input; the rule is a gate ──────────
    tf.invoke("/external/set_section_selection", "1:partial")
    tf.invoke("/external/set_section_selection", "2:full")
    wait_until(lambda: _h(tf, "selections")[2] == "full",
               desc="the consumer's coverage lands")
    published = ["none", "partial", "full", "none", "none"]
    assert_eq(_h(tf, "selections"), published,
              "the published row is what it was told")                         # 7
    assert_eq(_h(tf, "highlights"), NONE_ROW,
              "and NONE of it is painted, because the rule forbids it — the "
              "§2 #7 distinction a single row cannot express")                 # 8

    tf.intervene("/external/highlight_sections", True)
    wait_until(lambda: _h(tf, "highlight_sections") is True,
               desc="the rule turns on")
    assert_eq(_h(tf, "highlights"), published,
              "the same row is now painted, unchanged in content")             # 9
    assert_eq(_h(tf, "selections"), published,
              "and the input did not move when the gate did")                  # 10
    assert "| highlight on" in _readout(tf) and f"{HEADERS[2]}=full" in _readout(tf), \
        f"the readout names the rule and what it dresses: {_readout(tf)}"      # 11

    # ── (C) the paint follows the EFFECTIVE row ────────────────────
    # Both channels are node properties, so the tree is a real witness here —
    # unlike R1504's alignment, which the tree cannot show at all.
    fills_before = [_cell_fill(tf, v) for v in range(NCOLS)]
    assert_eq(_label_style(tf, 1)["font_weight"], WEIGHT_BOLD,
              "a partially-selected section bolds its label (the toolkit State_On)")    # 12
    assert_eq(_label_style(tf, 2)["font_weight"], WEIGHT_BOLD,
              "and so does a fully-covered one — coverage implies intersection") # 13
    assert_eq(_label_style(tf, 0)["font_weight"], WEIGHT_NORMAL,
              "an unselected section does not")                                # 14
    accent = _label_style(tf, 2)["fg_color"]
    assert accent != _label_style(tf, 1)["fg_color"], \
        "only FULL coverage accents the label (the toolkit State_Sunken): " \
        f"{accent} vs {_label_style(tf, 1)['fg_color']}"                       # 15
    assert_eq(_label_style(tf, 1)["fg_color"], _label_style(tf, 0)["fg_color"],
              "a partial highlight leaves the colour alone")                   # 16

    tf.intervene("/external/highlight_sections", False)
    wait_until(lambda: _h(tf, "highlights") == NONE_ROW,
               desc="the rule is revoked")
    assert_eq(_label_style(tf, 1)["font_weight"], WEIGHT_NORMAL,
              "revoking the rule un-bolds a label it had bolded")              # 17
    assert_eq(_label_style(tf, 2)["fg_color"], _label_style(tf, 0)["fg_color"],
              "and un-accents the covered one")                                # 18
    assert_eq([_cell_fill(tf, v) for v in range(NCOLS)], fills_before,
              "through all of which the section FILL never moved: it belongs "
              "to the drag and the keyboard cursor, so a highlight cannot "
              "hide the cursor")                                               # 19
    tf.intervene("/external/highlight_sections", True)
    wait_until(lambda: _h(tf, "highlights")[2] == "full", desc="the rule is back")

    # ── (D) the coverage travels with its column ───────────────────
    tf.invoke("/external/move_section", "2:0")
    wait_until(lambda: _h(tf, "order")[0] == 2, desc="Size dragged to the front")
    assert_eq(_h(tf, "section_highlight.2"), "full",
              "the coverage is keyed by column, so it went along")             # 20
    assert_eq(_h(tf, "highlights"), published,
              "the row is keyed logically and did not change shape")           # 21
    assert_eq(_label_style(tf, 0)["fg_color"], accent,
              "and the accent is now painted at the FRONT of the strip, "
              "where that column moved to")                                    # 22

    # ── (E) saved state carries the rule, not the selection ────────
    saved = _h(tf, "state")
    assert_eq(saved["highlight_sections"], True,
              "the rule is in the snapshot, as the toolkit's `highlightSelected` is")   # 23
    for absent in ("selections", "selection", "highlights"):
        assert absent not in saved, \
            f"{absent!r} must not be in a header snapshot: {sorted(saved)}"    # 24-26

    tf.intervene("/external/highlight_sections", False)
    wait_until(lambda: _h(tf, "highlight_sections") is False, desc="rule off")
    tf.intervene("/external/state", saved)
    wait_until(lambda: _h(tf, "highlight_sections") is True,
               desc="the state restores the rule")
    assert_eq(_h(tf, "selections"), published,
              "and the SELECTION survived the restore — it belongs to the "
              "view's selection model, which a header restore cannot reach "
              "(the opposite of R1504's alignment exceptions, which are "
              "header data and are dropped)")                                  # 27

    # ── (F) an older snapshot does not highlight ───────────────────
    tf.intervene("/external/state", _state_without_highlight(tf))
    wait_until(lambda: _h(tf, "highlight_sections") is False,
               desc="the older shape restores")
    assert_eq(_h(tf, "highlights"), NONE_ROW,
              "absent describes a header that had no selection input at all, "
              "and the toolkit starts in the same place — unlike sections_movable")     # 28
    assert "| highlight off" in _readout(tf), \
        f"and the readout says so: {_readout(tf)}"                             # 29
    assert_eq(_h(tf, "selections"), published,
              "the input is still there, still unpainted")                     # 30

    # ── (G) the gesture path reaches both ──────────────────────────
    # `apply_key` refuses a key aimed at anything but the strip, so the focus is
    # stated rather than assumed (the R1498 sibling's discipline).
    tf.request("focus/set", {"tag": HDR})
    assert_eq(tf.request("focus/get").result.get("focused"), HDR,
              "the strip has the keyboard")                                    # 31
    tf.key(path=f"{HDR}#0", name="H")
    wait_until(lambda: _h(tf, "highlight_sections") is True,
               desc="H toggles the rule")
    assert_eq(_h(tf, "highlight_sections"), True,
              "the header rule has a gesture, like `f` for stretchLastSection") # 30

    # `s` cycles the CURSOR's column: none -> partial -> full -> none.
    tf.invoke("/external/set_section_selection", "0:none")
    wait_until(lambda: _h(tf, "section_selection.0") == "none", desc="reset Name")
    _cycle_selection(tf, 0)
    wait_until(lambda: _h(tf, "section_selection.0") == "partial",
               desc="s selects part of the cursor's column")
    assert_eq(_h(tf, "section_selection.0"), "partial")                        # 31
    _cycle_selection(tf, 0)
    wait_until(lambda: _h(tf, "section_selection.0") == "full",
               desc="s again covers it")
    assert_eq(_h(tf, "section_selection.0"), "full")                           # 32
    _cycle_selection(tf, 0)
    wait_until(lambda: _h(tf, "section_selection.0") == "none",
               desc="s again clears it")
    assert_eq(_h(tf, "section_selection.0"), "none",
              "three presses is a full cycle, so the gesture can undo itself")  # 33

    # ── (H) refusals are typed ─────────────────────────────────────
    assert_action_refused(
        lambda: tf.invoke("/external/set_section_selection", "0:everything"),
        saying='"everything" is not a section-selection spelling',
    )                                                                          # 34
    assert_action_refused(
        lambda: tf.invoke("/external/set_section_selection", f"{NCOLS}:full"),
        saying=f"no section {NCOLS} in this header",
    )                                                                          # 35
    assert_rpc_error(
        lambda: tf.intervene("/external/highlight_sections", "yes"),
        data="InterveneTypeMismatch",
    )                                                                          # 36
    assert_out_of_range(
        lambda: tf.intervene("/external/selections", ["full"]),
        saying="needs 5 entries, not 1",
    )                                                                          # 37
    assert_rpc_error(
        lambda: tf.intervene("/external/selections", ["everything"] * NCOLS),
        data="InterveneTypeMismatch",
    )                                                                          # 38
    assert_eq(_h(tf, "section_selection.0"), "none",
              "and not one refusal changed anything")                          # 39
    # The whole row IS writable, like the two consumer-published inputs beside
    # it (§2 #2 — an input an agent can read but never move is unexplorable).
    tf.intervene("/external/selections", ["full"] * NCOLS)
    wait_until(lambda: _h(tf, "selections") == ["full"] * NCOLS,
               desc="the whole row is publishable over the wire")
    assert_eq(_h(tf, "highlights"), ["full"] * NCOLS)                          # 40
    # Out of range has no coverage at all — not a plausible "none" from inside
    # the domain (the R1501 defect).
    assert_eq(_h(tf, f"section_selection.{NCOLS}"), None)                      # 41
    assert_eq(_h(tf, f"section_highlight.{NCOLS}"), None)                      # 42

    # ── (I) the surface declares all of it ─────────────────────────
    schema = tf.query("/external/$schema")
    paths = {f["path"] for f in schema}
    for p in ("highlight_sections", "selections", "highlights",
              "section_selection.<logical>", "section_highlight.<logical>",
              "set_section_selection"):
        assert p in paths, f"{p!r} is declared: {sorted(paths)}"               # 43-48
    by_path = {f["path"]: f for f in schema}
    assert_eq(by_path["set_section_selection"].get("channel"), "invoke",
              "the action declares itself one rather than being told apart by "
              "the shape of its name")                                         # 49
    assert_eq(by_path["highlight_sections"].get("channel"), None,
              "and a readable path carries no channel key at all")             # 50
    assert_eq(
        [a["domain"] for a in by_path["section_highlight.<logical>"]["args"]],
        [{"kind": "index_of", "count_path": "count"}],
        "the family names the bound its argument indexes",
    )                                                                          # 51


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1510 §5.27 §2#2 §2#7 — header view highlightSections: the header "
        "dresses the sections the selection reaches",
        body,
    ))
