#!/usr/bin/env python3
"""R1491 §5.27 §5.40 §2#7 §2#2 — a header says which section it is sorted by.

Qt reference: `QHeaderView::setSortIndicator` / `sortIndicatorSection()` /
`sortIndicatorOrder()` / `setSortIndicatorShown()`, and the `saveState()` blob
that carries all three. Qt's header is movable AND clickable at once
(`setSectionsMovable` + `setSectionsClickable`): you drag a section to move it
and click it to sort, and the arrow travels with the column.

pinion had each half, in different demos, and never the pair. The reorderable
strip tagged its sections `colhdr#<visual>` for the drop classifier; the
sortable grid tagged its headers `{tag}_ch{col}` because "their click routes to
the table's own sort" (that sentence is in this demo's own module doc, as the
reason the two shapes were alternatives). So no pinion header could be dragged
and clicked, and `ColumnLayoutState` — which calls itself the peer of
`saveState()` — dropped a field `saveState()` carries.

The two tag shapes were never the obstacle. What separates a click from a drag
is the RELEASE: `ColumnLayout::release_section` commits the drop and reports the
clicked **logical** section only when the permutation came out unchanged, which
is Qt's own rule. The indicator then joins the sizes and the hidden flags as
logical-keyed header state, and moving a column re-aims its arrow without
touching the rows.

Where Qt cannot follow: `sortIndicatorSection()` is a C++ call whose result is
observable only by painting an arrow, and `saveState()` is an opaque versioned
`QByteArray`. Here the indicator is typed data both ways — readable as a
compound string AND as Qt's two separate getters, writable through the same
slot, invokable as `setSortIndicator` / a click's `cycle`, and carried inside
the readable snapshot.

What this asserts:

  (A) BOOT — the strip paints, the view has enabled sorting
      (`QTableView::setSortingEnabled`), nothing is sorted yet, and no arrow is
      painted anywhere.
  (B) A CLICK SORTS — a real `scene/drag` whose endpoints are the SAME section
      is a click: it sorts that section and moves nothing. Cycling it walks
      Qt's three states and reports where it landed.
  (C) A DRAG DOES NOT SORT — the same primitive aimed at a DIFFERENT section
      reorders and leaves the indicator alone. This is the half that makes (B)
      mean something: an implementation that always sorted would pass (B).
  (D) THE ARROW TRAVELS WITH ITS COLUMN — after a move, the glyph is painted on
      the sorted column's NEW position, its rect is inside that section, and
      the rows have not moved. A visually-keyed indicator paints on whatever is
      now first, so the painted x discriminates.
  (E) A CLICK AFTER A MOVE NAMES THE COLUMN — clicking the section now painted
      fourth sorts the column it shows, not the position.
  (F) THE ROWS FOLLOW — the body order is the indicator's, read off the PAINTED
      cells rather than the model, and reversing the direction bit reverses the
      rows exactly (which order alone cannot show).
  (G) SHOWN vs SECTION — hiding the arrow stops the paint and the `aria-sort`
      without unsorting the rows.
  (H) STATE — the snapshot carries the indicator, restores it, and a pre-R1491
      snapshot (no field at all) still restores as an unsorted header.
  (I) TYPED REFUSALS — a misspelled direction is reported rather than silently
      read as "unsorted", an out-of-range section is a DIFFERENT error, and
      neither moves the arrow.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1491_header_sort_indicator.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
SORT = "colhdr_sort"
BODY = "colbody"

HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
NROWS = 6
BOOT_W = [150, 90, 100, 130, 100]
IDENTITY = list(range(NCOLS))

# The Size column's cells, in source order — the numeric-aware sort's subject.
# Mirrors the binding's `SIZES` constant; the units are decoration, because the
# grid's comparator parses the LEADING NUMBER (`cell_cmp`), which is exactly
# what makes this column discriminate a numeric sort from a string one.
SIZES = ["2.1 MB", "880 KB", "4 KB", "1 KB", "32 KB", "1.4 GB"]


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    return None if node is None else node["rect"]


def _visual_of(tf, logical: int) -> int:
    return _h(tf, f"visual_index.{logical}")


def _arrow_visuals(tf) -> list[int]:
    """Which painted sections carry a sort glyph, read off the PAINT."""
    scene = _paint(tf)
    return [v for v in range(NCOLS) if find_by_tag(scene, f"{SORT}#{v}") is not None]


def _column_text(tf, logical: int) -> list[str]:
    """The painted cell text of one logical column, top to bottom."""
    scene = _paint(tf)
    visual = _visual_of(tf, logical)
    out = []
    for slot in range(NROWS):
        node = find_by_tag(scene, f"{BODY}#{slot}_{visual}")
        out.append(None if node is None else node["content"])
    return out


def _click_section(tf, visual: int) -> None:
    """A click, expressed as the drag primitive with both ends on one section.

    Deliberately the SAME `scene/drag` call (C) uses: if a click were a
    different RPC, the two would prove nothing about each other.
    """
    tf.drag(from_path=f"{HDR}#{visual}", to_path=f"{HDR}#{visual}", steps=4)


def _reset(tf) -> None:
    """Back to the boot layout through the restore path itself."""
    tf.intervene(
        "/external/state",
        {
            "order": IDENTITY,
            "sizes": BOOT_W,
            "hidden": [False] * NCOLS,
            "modes": ["interactive"] * NCOLS,
            "sort_indicator": "none",
            "sort_indicator_shown": True,
        },
    )
    wait_until(
        lambda: _h(tf, "order") == IDENTITY and _h(tf, "sort_indicator") == "none",
        desc="layout reset",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) boot: sortable, unsorted, no arrow ────────────────────
        wait_until(lambda: _rect(tf, f"{HDR}#0") is not None, desc="the strip paints")
        assert_eq(_h(tf, "sort_indicator"), "none", "boot: nothing is sorted")      # 1
        assert_eq(_h(tf, "sort_indicator_section"), None,
                  "and no section carries the indicator")                           # 2
        assert_eq(_h(tf, "sort_indicator_order"), "none",
                  "Qt's two getters agree with the compound string")                # 3
        # The view enabled sorting; `ColumnLayout` itself boots this false, as
        # Qt's QHeaderView does, so this asserts the binding made the call.
        assert_eq(_h(tf, "sort_indicator_shown"), True,
                  "the view turned the indicator on (setSortingEnabled)")           # 4
        assert_eq(_arrow_visuals(tf), [], "an unsorted header paints no arrow")     # 5
        assert_eq(_h(tf, "order"), IDENTITY, "boot order")                          # 6

        # ── (B) a click sorts the section it pressed ──────────────────
        _click_section(tf, 1)
        wait_until(lambda: _h(tf, "sort_indicator") == "1:ascending",
                   desc="the click sorted Type")                                    # 7
        assert_eq(_h(tf, "order"), IDENTITY, "and moved nothing at all")            # 8
        assert_eq(_h(tf, "sort_indicator_section"), 1, "section 1 by the getter")   # 9
        assert_eq(_h(tf, "sort_indicator_order"), "ascending", "ascending first")   # 10
        assert_eq(_arrow_visuals(tf), [1], "exactly one arrow, on Type")            # 11

        _click_section(tf, 1)
        wait_until(lambda: _h(tf, "sort_indicator") == "1:descending",
                   desc="the second click reverses")                                # 12
        _click_section(tf, 1)
        wait_until(lambda: _h(tf, "sort_indicator") == "none",
                   desc="the third click unsorts")                                  # 13
        assert_eq(_arrow_visuals(tf), [], "and takes the arrow away")               # 14

        # The wire's own cycle reports where it landed — the caller cannot know
        # the direction in advance, so returning it is the point.
        assert_eq(tf.invoke("/external/cycle_sort_indicator", 2), "2:ascending",
                  "cycle reports the state it reached")                             # 15

        # ── (C) a drag moves and does NOT sort ────────────────────────
        # Same primitive, different endpoint. Without this half, "always sort"
        # passes (B).
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#3", steps=6)
        wait_until(lambda: _h(tf, "order") == [1, 2, 3, 0, 4], desc="the drag commits")  # 16
        assert_eq(_h(tf, "sort_indicator"), "2:ascending",
                  "a release that reordered did not also sort what it dragged")     # 17

        # ── (D) the arrow travels with its column ─────────────────────
        # Size (logical 2) is sorted and now sits at visual 1.
        v_size = _visual_of(tf, 2)
        assert_eq(v_size, 1, "Size moved left when Name was dragged past it")       # 18
        assert_eq(_arrow_visuals(tf), [v_size],
                  "the arrow is on Size's NEW position, not on whatever is first")  # 19
        arrow = _rect(tf, f"{SORT}#{v_size}")
        section = _rect(tf, f"{HDR}#{v_size}")
        assert arrow is not None, "the glyph is a real painted node"                # 20
        assert section["x"] <= arrow["x"] and \
            arrow["x"] + arrow["w"] <= section["x"] + section["w"], \
            f"and it paints inside its own section: {arrow} vs {section}"           # 21

        # ── (E) a click after a move names the column ─────────────────
        # Visual 3 is logical 0 (Name). A click that answered the visual index
        # would sort column 3.
        _click_section(tf, 3)
        wait_until(lambda: _h(tf, "sort_indicator") == "0:ascending",
                   desc="the click named Name, not position 3")                     # 22
        assert_eq(_h(tf, "order"), [1, 2, 3, 0, 4], "still no reorder from a click")  # 23

        # ── (F) the rows follow the indicator ─────────────────────────
        _reset(tf)
        assert_eq(_column_text(tf, 2), SIZES, "unsorted: the source order")         # 24
        tf.intervene("/external/sort_indicator", "2:ascending")
        wait_until(lambda: _column_text(tf, 2) != SIZES, desc="the body re-sorts")
        by_size = _column_text(tf, 2)
        # The grid's shared comparator (`cell_cmp`) takes its numeric branch
        # only when BOTH cells parse whole as a number; these carry units, so it
        # falls to the documented string compare — the same answer Qt's default
        # `QSortFilterProxyModel` gives a display-role string. Asserted as what
        # it IS rather than as "numeric", which this column cannot show.
        assert_eq(by_size, sorted(SIZES),
                  "the painted rows ascend by the Size column")                     # 25
        # The discriminator for the direction bit, which #25 alone does not test:
        # an implementation that ignored `ascending` would pass #25 forever.
        tf.intervene("/external/sort_indicator", "2:descending")
        wait_until(lambda: _column_text(tf, 2) == list(reversed(by_size)),
                   desc="descending is the exact reverse")                          # 26
        tf.intervene("/external/sort_indicator", "2:ascending")
        wait_until(lambda: _column_text(tf, 2) == by_size, desc="and back")

        # A section move re-aims the arrow, NOT the rows: the sort names a
        # column, and that column's values did not change.
        tf.invoke("/external/move_section", "2:0")
        wait_until(lambda: _visual_of(tf, 2) == 0, desc="Size moves to the front")
        assert_eq(_column_text(tf, 2), by_size,
                  "moving the sorted column did not re-sort the rows")              # 27
        assert_eq(_arrow_visuals(tf), [0], "the arrow came with it")                # 28

        # ── (G) shown is presentation, the section is state ───────────
        tf.intervene("/external/sort_indicator_shown", False)
        wait_until(lambda: _arrow_visuals(tf) == [], desc="the arrow stops painting")  # 29
        assert_eq(_h(tf, "sort_indicator"), "2:ascending",
                  "hiding the arrow did not unsort the header")                     # 30
        assert_eq(_column_text(tf, 2), by_size, "and did not move the rows")        # 31
        tf.intervene("/external/sort_indicator_shown", True)
        wait_until(lambda: _arrow_visuals(tf) == [0], desc="the arrow returns")      # 32

        # ── (H) the readable snapshot carries it ──────────────────────
        saved = _h(tf, "state")
        assert_eq(saved["sort_indicator"], "2:ascending",
                  "saveState's peer names the sorted column, legibly")              # 33
        assert_eq(saved["sort_indicator_shown"], True, "and whether it is shown")   # 34
        _reset(tf)
        assert_eq(_h(tf, "sort_indicator"), "none", "reset cleared it")             # 35
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "sort_indicator") == "2:ascending",
                   desc="the snapshot restores the sort")                           # 36
        assert_eq(_h(tf, "order"), saved["order"], "along with the order")          # 37
        assert_eq(_column_text(tf, 2), by_size, "and the rows it implied")          # 38

        # A snapshot taken before this field existed is an OLDER shape, not a
        # malformed one — the same rule R1452's `modes` established.
        tf.intervene(
            "/external/state",
            {
                "order": IDENTITY,
                "sizes": BOOT_W,
                "hidden": [False] * NCOLS,
                "modes": ["interactive"] * NCOLS,
            },
        )
        wait_until(lambda: _h(tf, "sort_indicator") == "none",
                   desc="a pre-R1491 snapshot restores as unsorted")                # 39
        assert_eq(_h(tf, "order"), IDENTITY, "and the fields it DID carry landed")  # 40

        # ── (I) typed refusals ────────────────────────────────────────
        _reset(tf)
        tf.intervene("/external/sort_indicator", "3:descending")
        wait_until(lambda: _h(tf, "sort_indicator") == "3:descending",
                   desc="a well-formed sort lands")                                 # 41
        # Misspelled: reported, NOT silently read as "unsorted". The lenient
        # `grid_sort_from_str` would have unsorted the header and reported
        # success — which is why the strict parse exists.
        # (a misspelled direction is a SHAPE error, like a misspelled mode)
        assert_rpc_error(
            lambda: tf.intervene("/external/sort_indicator", "3:desending"),
            data="InterveneTypeMismatch",
        )                                                                           # 42
        # Well-formed but not this header's section — a DIFFERENT error, because
        # the client's mistake is a different mistake.
        assert_rpc_error(
            lambda: tf.intervene("/external/sort_indicator", "9:ascending"),
            data="OutOfRange",
        )                                                                           # 43
        # and the invoke half refuses it too, with its own typed variant
        assert_action_refused(
            lambda: tf.invoke("/external/cycle_sort_indicator", 9),
            saying="no section 9 in this header",
        )                                                                           # 44
        assert_eq(_h(tf, "sort_indicator"), "3:descending",
                  "no refusal moved the arrow")                                     # 45
        assert_eq(_arrow_visuals(tf), [3], "the paint agrees with the model")       # 46

        # A restore naming a section this header lacks is refused WHOLE — the
        # indicator's range check joins the length checks ahead of any write.
        assert_rpc_error(
            lambda: tf.intervene(
                "/external/state",
                {
                    "order": [4, 3, 2, 1, 0],
                    "sizes": [10, 20, 30, 40, 50],
                    "hidden": [True] * NCOLS,
                    "modes": ["stretch"] * NCOLS,
                    "sort_indicator": "9:ascending",
                    "sort_indicator_shown": True,
                },
            ),
            data="OutOfRange",
        )                                                                           # 47
        assert_eq(_h(tf, "order"), IDENTITY, "and not one field of it was written")  # 48
        assert_eq(_h(tf, "sort_indicator"), "3:descending", "the sort held too")    # 49


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1491 §5.27 §5.40 §2#7 — QHeaderView sort indicator: click, move, save",
        body,
    ))
