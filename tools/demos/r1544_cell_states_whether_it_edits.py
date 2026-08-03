#!/usr/bin/env python3
"""R1544 §5.27 §5.40 — the delegate's EDITING half, over the wire.

R1532 gave the virtualized grid a per-column *paint* delegate; the other
half of Qt's `QStyledItemDelegate` — `createEditor` / `setEditorData` /
`setModelData` — did not exist, so the grid's cell path could not host an
editor at all. This drives `hello-grid-nav`, now the editable Model/View
grid at scale, and proves each piece from the outside:

  (A) the model's `Qt::EditRole` decides editability — the identity column
      answers `None`, so no trigger opens an editor on it, and every one of
      its cells is `aria-readonly` in `scene/access` while its neighbours
      are silent (WAI-ARIA's default-false). Qt says nothing here at all:
      `QAccessibleTableCell` builds its state from the view's selection,
      never from the model's `Qt::ItemIsEditable`.
  (B) the trigger set (Qt `EditTriggers`) — <kbd>F2</kbd> (`EditKeyPressed`)
      and a printable key (`AnyKeyPressed`) open an editor on the current
      CELL; a double-click (`DoubleClicked`) opens one on the clicked cell.
  (C) the editor replaces exactly one cell — the editing cell hosts the
      inline field while its row-neighbour and its column-peer keep the
      display painter.
  (D) the editor DELEGATE — the bounded column opens an editor that states
      its bound (`/100`), which the built-in editor does not paint.
  (E) commit writes through, and a **refused** write keeps the editor open
      holding what was typed. This is the divergence from Qt: there,
      `setModelData` returns `void`, so a rejected value closes the editor
      and the typing is discarded with no feedback.
  (F) <kbd>Escape</kbd> abandons the edit and the model is unchanged.
  (G) <kbd>Tab</kbd> is Qt's `EditNextItem`: commit, then open an editor on
      the next EDITABLE cell — skipping the read-only column.
  (H) the editing state is DATA (§2 #7): the status line reports which cell
      is open and what is in it. Qt has no public equivalent — a transient
      editor's buffer lives inside an opaque `QWidget`.

ZERO-FLAKE: bounded `wait_snap` / `wait_until` polling, never a fixed
sleep. >=30 assertions.

Run from the workspace root:
    cargo build -p hello-grid-nav --release
    python3 tools/demos/r1544_cell_states_whether_it_edits.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
    wait_snap,
    wait_until,
)

WIN = (470, 480)
TABLE_TAG = "vtbl"
STATUS_TAG = "vtbl_status"
EDIT_FIELD_TAG = "vtbl_editor"
# Column roles, mirroring the binding's constants.
INDEX_COL = 0
NAME_COL = 1
SCORE_COL = 2
SCORE_MAX = 100


def cell_tag(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def cell_texts(snap: Any, row: int, col: int) -> list[str]:
    node = find_by_tag(snap, cell_tag(row, col))
    assert node is not None, f"cell {row}_{col} is painted"
    return texts_of(node)


def has_editor(snap: Any, row: int, col: int) -> bool:
    """Whether the inline editor field is inside this cell's subtree."""
    node = find_by_tag(snap, cell_tag(row, col))
    return node is not None and find_by_tag(node, EDIT_FIELD_TAG) is not None


def status(snap: Any) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "status line painted"
    return " ".join(texts_of(node))


def editing_field(snap: Any) -> Optional[str]:
    """The `editing <cell> "<text>"` clause of the status line, or None."""
    text = status(snap)
    marker = "editing "
    at = text.find(marker)
    return None if at < 0 else text[at + len(marker) :].strip()


def read_only_tags(access: Any, row: int) -> set[int]:
    """Columns of `row` whose `gridcell` claims `aria-readonly`."""
    out: set[int] = set()
    for col in (INDEX_COL, NAME_COL, SCORE_COL):
        node = access_node_by_tag(access, cell_tag(row, col))
        if node is None:
            continue
        if (node.get("state") or {}).get("read_only") is True:
            out.add(col)
    return out


def body() -> None:
    with RpcSubprocess("hello-grid-nav", boot_grace=1.5) as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(0, SCORE_COL)) is not None,
            desc="grid paints its cells at boot",
            viewport=WIN,
        )
        assert_eq(editing_field(snap), "none", "no editor is open at boot")
        assert not has_editor(snap, 0, NAME_COL), "and no cell hosts a field"

        # ── (A) the model's EditRole decides editability ─────────────
        access = tf.request("scene/access").result
        assert_eq(
            read_only_tags(access, 0),
            {INDEX_COL},
            "only the identity column is aria-readonly — the editable ones "
            "stay silent, which is WAI-ARIA's default-false",
        )
        assert_eq(
            read_only_tags(access, 3), {INDEX_COL}, "the same holds on another row"
        )
        index_cell = access_node_by_tag(access, cell_tag(0, INDEX_COL))
        assert index_cell is not None, "the read-only cell is still IN the AT tree"
        assert (index_cell.get("state") or {}).get("disabled") is not True, (
            "read-only is not disabled: the cell stays reachable and copyable"
        )

        tf.request("focus/set", {"tag": TABLE_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == TABLE_TAG,
            desc="grid owns focus",
        )

        # The current cell starts at (0, 0) — the read-only column, so the
        # edit key opens nothing. Editability is the MODEL's answer, not a
        # second check in the trigger path.
        tf.key(path=TABLE_TAG, name="F2")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            editing_field(snap), "none", "F2 on the read-only column opens nothing"
        )
        assert not has_editor(snap, 0, INDEX_COL), "and paints no field there"

        # ── (B) Qt EditKeyPressed on an editable cell ───────────────
        tf.key(path=TABLE_TAG, name="ArrowRight")
        tf.key(path=TABLE_TAG, name="F2")
        snap = wait_snap(
            tf,
            lambda s: has_editor(s, 0, NAME_COL),
            desc="F2 opens an editor on the current cell",
            viewport=WIN,
        )
        assert_eq(
            editing_field(snap),
            '0_1 "Alpha"',
            "the latch names the cell and the editor is seeded from the model",
        )

        # ── (C) an editor replaces exactly ONE cell ─────────────────
        assert has_editor(snap, 0, NAME_COL), "the editing cell hosts the field"
        assert not has_editor(snap, 0, INDEX_COL), "its row-neighbour does not"
        assert not has_editor(snap, 0, SCORE_COL), "nor its other neighbour"
        assert not has_editor(snap, 1, NAME_COL), "nor the same column one row down"
        assert find_by_tag(snap, cell_tag(0, NAME_COL)) is not None, (
            "the cell keeps its own tag while editing, so it stays addressable"
        )

        # ── (F) Escape abandons; the model is untouched ─────────────
        before = cell_texts(snap, 0, NAME_COL)
        tf.key(path=TABLE_TAG, name="Escape")
        snap = wait_snap(
            tf,
            lambda s: not has_editor(s, 0, NAME_COL),
            desc="Escape closes the editor",
            viewport=WIN,
        )
        assert_eq(editing_field(snap), "none", "the latch is clear")
        assert_eq(cell_texts(snap, 0, NAME_COL), before, "the model is unchanged")

        # ── (B') Qt AnyKeyPressed — type-to-replace ─────────────────
        tf.key(path=TABLE_TAG, name="Z")
        snap = wait_snap(
            tf,
            lambda s: has_editor(s, 0, NAME_COL),
            desc="a printable key opens the editor (Qt AnyKeyPressed)",
            viewport=WIN,
        )
        assert_eq(
            editing_field(snap),
            '0_1 "Z"',
            "the seed was fully selected on open, so the keystroke REPLACED it",
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: not has_editor(s, 0, NAME_COL),
            desc="Enter commits and closes",
            viewport=WIN,
        )
        assert "Z" in cell_texts(snap, 0, NAME_COL), (
            f"the committed value is what the grid paints: {cell_texts(snap, 0, NAME_COL)}"
        )
        assert_eq(editing_field(snap), "none", "and the latch is clear")

        # ── (B'') Qt DoubleClicked on the bounded column ────────────
        tf.double_click(path=cell_tag(2, SCORE_COL))
        snap = wait_snap(
            tf,
            lambda s: has_editor(s, 2, SCORE_COL),
            desc="a double-click opens an editor on the clicked cell",
            viewport=WIN,
        )
        opened = editing_field(snap)
        assert opened is not None and opened.startswith("2_2 "), (
            f"the latch names the double-clicked cell, got {opened!r}"
        )

        # ── (D) the editor DELEGATE paints the column's bound ───────
        painted = cell_texts(snap, 2, SCORE_COL)
        assert f"/{SCORE_MAX}" in painted, (
            f"the delegate's range hint is painted: {painted}"
        )

        # ── (E) a REFUSED commit keeps the editor open ──────────────
        before_score = None
        for _ in range(4):
            tf.key(path=TABLE_TAG, name="Backspace")
        tf.text("500", path=TABLE_TAG)
        snap = wait_snap(
            tf,
            lambda s: editing_field(s) == '2_2 "500"',
            desc="the out-of-range value is typed into the editor",
            viewport=WIN,
        )
        before_score = [t for t in cell_texts(snap, 2, SCORE_COL) if t != f"/{SCORE_MAX}"]
        tf.key(path=TABLE_TAG, name="Enter")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert has_editor(snap, 2, SCORE_COL), (
            "the model refused 500, so the editor is STILL OPEN — Qt's void "
            "setModelData cannot express this and closes anyway"
        )
        assert_eq(
            editing_field(snap),
            '2_2 "500"',
            "holding exactly what the user typed, so they can correct it",
        )

        # Correcting it in place commits.
        tf.key(path=TABLE_TAG, name="Backspace")
        snap = wait_snap(
            tf,
            lambda s: editing_field(s) == '2_2 "50"',
            desc="a correcting keystroke lands in the still-open editor",
            viewport=WIN,
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: not has_editor(s, 2, SCORE_COL),
            desc="the in-range value commits",
            viewport=WIN,
        )
        assert "50" in cell_texts(snap, 2, SCORE_COL), (
            f"the corrected value is painted: {cell_texts(snap, 2, SCORE_COL)}"
        )
        assert before_score != [
            t for t in cell_texts(snap, 2, SCORE_COL) if t != f"/{SCORE_MAX}"
        ], "and it differs from what the cell held before"

        # A malformed value is refused the same way.
        tf.double_click(path=cell_tag(2, SCORE_COL))
        wait_snap(
            tf,
            lambda s: has_editor(s, 2, SCORE_COL),
            desc="reopen the bounded cell",
            viewport=WIN,
        )
        for _ in range(4):
            tf.key(path=TABLE_TAG, name="Backspace")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            editing_field(snap), '2_2 ""', "the int editor's buffer is now empty"
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert has_editor(snap, 2, SCORE_COL), (
            "an unparseable commit is refused too, and the editor stays open"
        )
        tf.key(path=TABLE_TAG, name="Escape")
        wait_snap(
            tf,
            lambda s: not has_editor(s, 2, SCORE_COL),
            desc="Escape leaves the refused edit",
            viewport=WIN,
        )

        # The int editor's keystroke gate: a letter never reaches the buffer.
        tf.double_click(path=cell_tag(3, SCORE_COL))
        wait_snap(
            tf,
            lambda s: has_editor(s, 3, SCORE_COL),
            desc="open the bounded cell on another row",
            viewport=WIN,
        )
        gated = editing_field(tf.snapshot(source="paint", viewport=WIN))
        tf.key(path=TABLE_TAG, name="q")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            editing_field(snap),
            gated,
            "a letter is gated out of an int editor (CellKind::accepts_keystroke)",
        )

        # ── (G) Tab is Qt's EditNextItem, and it skips read-only ────
        tf.key(path=TABLE_TAG, name="Tab")
        snap = wait_snap(
            tf,
            lambda s: has_editor(s, 4, NAME_COL),
            desc="Tab commits and opens the NEXT editable cell",
            viewport=WIN,
        )
        moved = editing_field(snap)
        assert moved is not None and moved.startswith("4_1 "), (
            f"row 3's last editable cell advances to row 4's Name, skipping "
            f"row 4's read-only Index column entirely, got {moved!r}"
        )
        assert not has_editor(snap, 4, INDEX_COL), (
            "the read-only column never hosts an editor"
        )
        assert not has_editor(snap, 3, SCORE_COL), "the previous editor closed"

        # Shift+Tab is EditPreviousItem — back to where it came from.
        tf.modifiers(shift=True)
        tf.key(path=TABLE_TAG, name="Tab")
        tf.modifiers(shift=False)
        snap = wait_snap(
            tf,
            lambda s: has_editor(s, 3, SCORE_COL),
            desc="Shift+Tab walks back to the previous editable cell",
            viewport=WIN,
        )
        back = editing_field(snap)
        assert back is not None and back.startswith("3_2 "), (
            f"Qt EditPreviousItem lands on row 3's Score, got {back!r}"
        )
        tf.key(path=TABLE_TAG, name="Escape")

        # ── (H) the editing state is DATA, and the AT tree agrees ───
        snap = wait_snap(
            tf,
            lambda s: editing_field(s) == "none",
            desc="the grid returns to a non-editing state",
            viewport=WIN,
        )
        access = tf.request("scene/access").result
        assert_eq(
            read_only_tags(access, 4),
            {INDEX_COL},
            "the editability claim is unchanged by a round of editing",
        )
        grid = access_node_by_tag(access, TABLE_TAG)
        assert grid is not None, "the grid container is still in the AT tree"
        assert (grid.get("state") or {}).get("read_only") is not True, (
            "editability is claimed per cell, not blanket on the container"
        )
        # Selection stayed a separate axis throughout (Qt: current != selected).
        assert tf.query("/external/item_count") == 10_000, (
            "the coordinator still holds the full dataset — editing is windowed"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1544 §5.27 §5.40 the delegate's editing half", body))
