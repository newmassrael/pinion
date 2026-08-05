#!/usr/bin/env python3
"""R1450 §5.27 §5.40 §5.51 §2#7 — a column moves where its header is dragged.

Qt reference: `QHeaderView` movable sections — `setSectionsMovable`,
`moveSection(from, to)`, `visualIndex` <-> `logicalIndex`, and
`saveState`/`restoreState`. Every other column axis was already in tree (width,
visibility, sort, filter, frozen panes); section ORDER was the one Qt had and
pinion did not, and it is the mapping all the others have to compose with.

And one place Qt is weaker: a header layout persists as `QHeaderView::saveState()`
— an opaque versioned `QByteArray`. An agent cannot read "which column is third
now" out of it, and cannot write one back without a live QHeaderView. Here the
permutation is typed data in both directions.

What this asserts:

  (A) BOOT — the strip paints five sections, the body paints their data, and the
      order readout is the identity.
  (B) A REAL DRAG — `scene/drag` presses a header and marches the cursor, the
      same arc a mouse takes. The column moves, AND the BODY MOVES WITH IT: the
      cell text under visual position 2 is the dragged column's data. A header
      that moved without its data would pass every order assertion and still be
      broken, so this is the assertion that matters.
  (C) THE LIVE DROP TARGET — a drag HELD mid-gesture (`phase="begin"`) publishes
      its `preview` and paints the insertion line, so an agent can see where a
      drop would land before committing it.
  (D) QT'S MAPPING — `visual_index.<logical>` and `logical_index.<visual>` invert
      each other for every column, which is what makes them a mapping rather
      than two readouts that happen to agree.
  (E) moveSection OVER THE WIRE — the Qt-named operation, returning the resulting
      order in one round-trip.
  (F) RESTORE — a whole saved permutation goes back over `scene/intervene`
      (Qt restoreState). The refusals are typed: a malformed value is a
      TypeMismatch, a well-formed non-permutation is OutOfRange, and neither
      changes the layout.
  (G) THE KEYBOARD — APG pick-up: arrows rove, Space grabs, an arrow moves the
      grabbed section, Escape reverts to the pre-grab order.
  (H) THE AT TREE — `scene/access` announces the columnheaders in VISUAL order,
      because that is what the screen shows.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1450_column_reorder.py
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
ORDER_TAG = "colreorder_order"

HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
# Row 0 of the source model, by LOGICAL column — what the body must carry along
# when its header moves.
ROW0 = ["report.pdf", "PDF", "2.1 MB", "2026-06-01", "coin"]


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _present(snap, tag: str) -> bool:
    return find_by_tag(snap, tag) is not None


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _painted_headers(tf) -> list[str]:
    """The header labels the strip is actually painting, left to right."""
    snap = _paint(tf)
    out = []
    for v in range(NCOLS):
        node = find_by_tag(snap, f"colhdr_label#{v}")
        assert node is not None, f"section {v} paints a label"
        out.append(node.get("content") or "")
    return out


def _painted_row0(tf) -> list[str]:
    """The body's row-0 cell text, left to right."""
    snap = _paint(tf)
    out = []
    for v in range(NCOLS):
        node = find_by_tag(snap, f"colbody#0_{v}")
        assert node is not None, f"body cell 0,{v} paints"
        out.append(node.get("content") or "")
    return out


def _order(tf) -> list[int]:
    return _h(tf, "order")


def _access(tf):
    nodes = tf.request("scene/access").result["nodes"]
    return nodes


def _colheader_names(tf) -> list[str]:
    return [n.get("name") for n in _access(tf) if n.get("role") == "columnheader"]


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        wait_until(lambda: _present(_paint(tf), f"{HDR}#0"), desc="the strip paints")
        assert_eq(_h(tf, "count"), NCOLS, "boot: five sections")                        # 1
        assert_eq(_order(tf), [0, 1, 2, 3, 4], "boot: identity order")                  # 2
        assert_eq(_h(tf, "labels"), HEADERS, "boot: labels in schema order")            # 3
        assert_eq(_painted_headers(tf), HEADERS, "and that is what is painted")         # 4
        assert_eq(_painted_row0(tf), ROW0, "boot: body row 0 in schema order")          # 5
        assert "order Name Type Size" in find_by_tag(_paint(tf), ORDER_TAG)["content"], \
            "the order readout is scene-as-data"                                        # 6
        assert not _present(_paint(tf), "colhdr_dropline"), "no drop line at rest"      # 7

        # ── (C) a HELD drag publishes where the drop would land ──────
        # (run before the committing drag so the identity order is the baseline)
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#2", steps=6, phase="begin")
        wait_until(lambda: _h(tf, "preview") is not None,
                   desc="the held drag publishes a preview")
        preview = _h(tf, "preview")
        assert_eq(preview["from_visual"], 0, "the preview names the dragged section")   # 8
        assert_eq(preview["insert_at"], 3,
                  "the cursor is past section 2's midpoint, so the gap is 3")           # 9
        assert _present(_paint(tf), "colhdr_dropline"), "and the line paints"           # 10
        assert_eq(_order(tf), [0, 1, 2, 3, 4], "a held drag has not moved anything")    # 11

        # ── (B) settle it: the column moves, and the BODY comes along ─
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#2", steps=6, phase="end")
        wait_until(lambda: _order(tf) == [1, 2, 0, 3, 4], desc="the drag commits")      # 12
        assert_eq(_h(tf, "preview"), None, "the preview clears on release")             # 13
        assert not _present(_paint(tf), "colhdr_dropline"), "and so does the line"      # 14
        assert_eq(_painted_headers(tf), ["Type", "Size", "Name", "Modified", "Owner"],
                  "Name is painted third now")                                          # 15
        assert_eq(_painted_row0(tf),
                  ["PDF", "2.1 MB", "report.pdf", "2026-06-01", "coin"],
                  "THE BODY MOVED WITH ITS HEADER")                                     # 16
        assert_eq(_painted_row0(tf)[2], ROW0[0],
                  "the third visual column carries the dragged column's data")          # 17

        # ── (D) Qt's mapping, both directions ────────────────────────
        assert_eq(_h(tf, "visual_index.0"), 2, "Name's visual index is 2")              # 18
        assert_eq(_h(tf, "logical_index.2"), 0, "and position 2 holds Name")            # 19
        for logical in range(NCOLS):
            v = _h(tf, f"visual_index.{logical}")
            assert_eq(_h(tf, f"logical_index.{v}"), logical,
                      f"the mapping inverts for column {logical}")                      # 20-24
        assert_eq(_h(tf, "visual_index.9"), None, "out of range is present-but-empty")  # 25
        assert_eq(_h(tf, "logical_index.9"), None, "in both directions")                # 26

        # ── (E) moveSection over the wire ────────────────────────────
        out = tf.invoke(f"/external/move_section", "2:0")
        assert_eq(out, [0, 1, 2, 3, 4],
                  "moveSection returns the resulting order, and undoes the drag")       # 27
        assert_eq(_painted_headers(tf), HEADERS, "the strip is back to schema order")   # 28
        assert_eq(_painted_row0(tf), ROW0, "and so is the body")                        # 29
        # R1564 — an out-of-range target and a malformed pair were the same
        # frame; they are different mistakes with different fixes.
        assert_action_refused(
            lambda: tf.invoke("/external/move_section", "0:9"),
            saying="0 -> 9 is outside this model",
        )                                                                          # 30
        assert_action_refused(
            lambda: tf.invoke("/external/move_section", "nope"),
            saying='malformed argument "nope"',
        )                                                                          # 31

        # ── (F) restoreState — a whole permutation, as typed data ─────
        tf.intervene(f"/external/order", [4, 3, 2, 1, 0])
        wait_until(lambda: _order(tf) == [4, 3, 2, 1, 0], desc="the layout restores")   # 32
        assert_eq(_painted_headers(tf), ["Owner", "Modified", "Size", "Type", "Name"],
                  "the reversed layout is what is painted")                             # 33
        assert_eq(_painted_row0(tf), list(reversed(ROW0)), "body reversed with it")      # 34
        # The refusals are typed, and neither of them moves the layout. Without
        # this, a silently-accepted bad permutation would corrupt the mapping
        # every assertion above depends on.
        assert_rpc_error(lambda: tf.intervene(f"/external/order", [0, 0, 1, 2, 3]),
                         data="OutOfRange")                                             # 35
        assert_rpc_error(lambda: tf.intervene(f"/external/order", 3),
                         data="InterveneTypeMismatch")                                  # 36
        assert_rpc_error(lambda: tf.intervene(f"/external/labels", "x"),
                         data="ReadOnly")                                               # 37
        assert_eq(_order(tf), [4, 3, 2, 1, 0], "three refusals, nothing moved")         # 38

        # ── (G) the keyboard pick-up ─────────────────────────────────
        tf.intervene(f"/external/order", [0, 1, 2, 3, 4])
        wait_until(lambda: _order(tf) == [0, 1, 2, 3, 4], desc="reset for the keyboard")
        tf.request("focus/set", {"tag": HDR})
        assert_eq(tf.request("focus/get").result.get("focused"), HDR, "the strip has focus")  # 39
        tf.key(path=f"{HDR}#0", name="ArrowRight")
        wait_until(lambda: _h(tf, "focused_index") == 0, desc="the cursor lands on 0")   # 40
        tf.key(path=f"{HDR}#0", name="ArrowRight")
        wait_until(lambda: _h(tf, "focused_index") == 1, desc="and roves to 1")          # 41
        tf.key(path=f"{HDR}#1", name=" ")
        wait_until(lambda: _h(tf, "grabbed") is True, desc="Space picks the section up") # 42
        tf.key(path=f"{HDR}#1", name="ArrowRight")
        wait_until(lambda: _order(tf) == [0, 2, 1, 3, 4], desc="the grabbed section moves")  # 43
        assert_eq(_painted_headers(tf), ["Name", "Size", "Type", "Modified", "Owner"],
                  "the strip shows the keyboard move")                                   # 44
        tf.key(path=f"{HDR}#2", name="Escape")
        wait_until(lambda: _order(tf) == [0, 1, 2, 3, 4], desc="Escape reverts the grab") # 45
        assert_eq(_h(tf, "grabbed"), False, "and drops the grab")                        # 46

        # ── (H) the AT tree reads the screen, not the schema ──────────
        tf.invoke(f"/external/move_section", "4:0")
        wait_until(lambda: _order(tf) == [4, 0, 1, 2, 3], desc="Owner moves to the front")
        assert_eq(_colheader_names(tf),
                  ["Owner", "Name", "Type", "Size", "Modified"],
                  "scene/access announces the columnheaders in VISUAL order")            # 47
        assert_eq(len(_colheader_names(tf)), NCOLS, "one columnheader per section")       # 48
        rows = [n for n in _access(tf) if n.get("role") == "row"]
        assert_eq(len(rows), 1, "the strip itself is the header row")                     # 49
        assert_eq(rows[0].get("name"), "Columns", "and it is named")                       # 50


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1450 §5.51 §2#7 — QHeaderView movable sections: drag, move, restore",
        body,
    ))
