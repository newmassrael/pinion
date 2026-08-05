#!/usr/bin/env python3
"""R1563 §5.27 §5.40 — a selection has two axes.

Drives the `hello-column-select` binding (10 000 rows x 8 columns, virtualized
on both axes, Qt `SelectionBehavior::SelectItems`) over JSON-RPC, and reads the
row-select `hello-grid-multi-select` beside it for the refusal.

R1561 made a selection a set of runs over ROWS. R1562 made the vertical band's
section select the row through it, and named the mirror it could not build: a
COLUMN header cannot select its column, because there was no column axis to
select on. "Column 3" was not a statement the windowed coordinator could hold
at any price.

  (A) boot — the column axis is DECLARED, and both press policies are readable.
  (B) a column-header press selects the whole column: 10 000 cells, ONE band.
  (C) Shift widens the column span; Ctrl takes one column back out.
  (D) a cell press selects ONE CELL, not its row (Qt `SelectItems`).
  (E) Shift on a cell is a RECTANGLE from the extension origin.
  (F) a partly selected row is a state the row axis could not hold: `cells`
      says so, and the two counts (cells vs bands) stop being one question.
  (K) scattered Ctrl-presses sharing a column set MERGE into one band, and
      the same cells reached in either order are the same VALUE.
  (G) `All` outlives its count — a row selected AS A RECORD and a row whose
      eight columns are named individually paint the same and are DIFFERENT
      VALUES, which is the whole of what Qt cannot say.
  (H) it reaches assistive technology: the cell, the column header, and NOT
      the row.
  (I) a grid with no column axis REFUSES by name rather than doing nothing.
  (J) the verbs are DECLARED — `$schema` says what can be called, not only
      what can be read.

Against Qt 6.11:
  * `QItemSelectionRange` spans rows and columns, so the CAPABILITY is Qt's
    floor.  What is past it is that a full row here is `"all"` — a statement
    with no column count in it.  A Qt full row is
    `QItemSelectionRange(index(r, 0), index(r, columnCount() - 1))`, bound to
    the width at the moment of selection, so inserting a column silently
    demotes it and `selectedRows()` stops returning it.  Phase (G).
  * `QItemSelection` is a `QList<QItemSelectionRange>` that permits overlap,
    which is why `selectedIndexes()` must promise de-duplication — so two Qt
    selections covering the same cells can differ as values by the order the
    `select()` calls arrived in.  Phases (B)/(C) read a canonical band set
    instead, and (F) reads the two counts a canonical form makes cheap.
  * `QHeaderView` reaches column selection through `sectionPressed` while
    `sectionClicked` drives `sortByColumn`, with nothing declaring which a
    given header has.  Phase (A) reads the declaration.
  * `QAbstractItemView::selectColumn` returns `void`; a call on a view that
    cannot do it is lost.  Phase (I).
  * `QAccessibleTableHeaderCell` has no selection state of any kind, so a
    fully selected Qt column announces exactly as an unselected one.  Phase
    (H).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_eq,
    chord_click,
    find_by_tag,
    indexed_tags,
    run_demo,
    text_of_tag,
    wire_bytes,
)

EXAMPLE = "hello-column-select"
ROW_SELECT_EXAMPLE = "hello-grid-multi-select"
WIN = (560, 480)
N = 10_000
NCOLS = 8
TABLE_TAG = "vtbl"
STATUS_TAG = "vtbl_status"

#: What a whole-column selection may cost on the wire. The fact is
#: `[{"rows": [[0, 9999]], "columns": [[3, 3]]}]`.
MAX_BAND_BYTES = 96


def header(col: int) -> str:
    """The press address of one column-header section."""
    return f"{TABLE_TAG}#h{col}"


def cell(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def band_row(row: int) -> str:
    return f"{TABLE_TAG}#r{row}"


def cells(tf) -> list:
    """The two-axis selection as it arrives: the bands themselves."""
    return tf.query("/external/cells")






def painted_headers(tf) -> list[int]:
    """Which column-header sections the band actually drew."""
    snap = tf.snapshot(source="paint", viewport=WIN)
    return sorted(indexed_tags(abs_rects_of(snap), f"{TABLE_TAG}#h"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert find_by_tag(snap, TABLE_TAG) is not None, "grid present at boot"

        # ── (A) the column axis is declared ──────────────────────────
        assert_eq(tf.query("/external/mode"), "multi", "a multi-select model")
        assert_eq(tf.query("/external/item_count"), N, "over the whole dataset")
        assert_eq(
            tf.query("/external/column_count"),
            NCOLS,
            "and it declares its WIDTH — the gate on the whole column axis",
        )
        assert_eq(
            tf.query("/external/behavior"),
            "items",
            "a press selects a cell (Qt SelectionBehavior::SelectItems)",
        )
        assert_eq(
            tf.query("/external/section_press"),
            "select",
            "and a header-section press selects its column — DECLARED, where "
            "Qt leaves it to whoever connected sectionPressed",
        )
        assert_eq(cells(tf), [], "nothing selected at boot")
        assert_eq(tf.query("/external/cell_count"), 0, "no cells")
        assert_eq(tf.query("/external/band_count"), 0, "and no bands to hold them")
        drawn = painted_headers(tf)
        assert drawn, "the horizontal band drew sections"
        assert header(drawn[0]).endswith(f"#h{drawn[0]}"), "each carries its address"

        # ── (B) a header press selects the whole column ──────────────
        chord_click(tf, header(3))
        got = cells(tf)
        assert_eq(
            got,
            [{"rows": [[0, N - 1]], "columns": [[3, 3]]}],
            "the section selected the column through it",
        )
        assert_eq(len(got), 1, "ten thousand cells, ONE band")
        assert_eq(tf.query("/external/band_count"), 1, "and it says so")
        assert_eq(tf.query("/external/cell_count"), N, "the extension is the model's height")
        assert_eq(
            tf.query("/external/column_selection"),
            [[3, 3]],
            "Qt selectedColumns(), which there costs one QModelIndex per row",
        )
        assert wire_bytes(got) <= MAX_BAND_BYTES, (
            f"a whole column must cost at most {MAX_BAND_BYTES} bytes, "
            f"got {wire_bytes(got)}"
        )
        assert_eq(
            tf.query("/external/selection"),
            [],
            "and NO row is selected as a record — one of eight columns is not a row",
        )

        # ── (C) Shift widens the span; Ctrl takes one back ───────────
        chord_click(tf, header(5), shift=True)
        assert_eq(
            cells(tf),
            [{"rows": [[0, N - 1]], "columns": [[3, 5]]}],
            "Shift extends along the COLUMN axis, from the same anchor",
        )
        assert_eq(tf.query("/external/column_selection"), [[3, 5]], "three full columns")
        assert_eq(tf.query("/external/cell_count"), 3 * N, "and the cells to match")
        chord_click(tf, header(4), ctrl=True)
        assert_eq(
            tf.query("/external/column_selection"),
            [[3, 3], [5, 5]],
            "Ctrl took the middle column back out, leaving two runs",
        )
        assert_eq(tf.query("/external/band_count"), 1, "still one band — one column set")

        # ── (D) a cell press selects one CELL ────────────────────────
        chord_click(tf, cell(2, 1))
        assert_eq(
            cells(tf),
            [{"rows": [[2, 2]], "columns": [[1, 1]]}],
            "the press replaced the selection with that ONE cell",
        )
        assert_eq(tf.query("/external/cell_count"), 1, "one cell")
        assert_eq(tf.query("/external/selected"), 2, "the active row is its row")
        assert_eq(tf.query("/external/selected_column"), 1, "and the active COLUMN is its own")
        assert_eq(
            tf.query("/external/selection"),
            [],
            "its row is not selected — Qt SelectRows would have taken all eight",
        )

        # ── (E) Shift on a cell is a rectangle ───────────────────────
        chord_click(tf, cell(5, 4), shift=True)
        assert_eq(
            cells(tf),
            [{"rows": [[2, 5]], "columns": [[1, 4]]}],
            "the rectangle from the extension origin (Qt QItemSelectionRange)",
        )
        assert_eq(tf.query("/external/cell_count"), 4 * 4, "four rows by four columns")
        assert_eq(tf.query("/external/band_count"), 1, "one column set, so one band")

        # ── (F) a partly selected row, and two counts ────────────────
        # A state the row axis could not hold at all: row 2 has four of eight
        # columns. `cell_count` is the extension, `band_count` the size of the
        # statement — on a one-axis selection those were one question.
        assert_eq(
            tf.query("/external/anchor"),
            2,
            "the extension origin stayed put through the Shift",
        )
        assert_eq(tf.query("/external/anchor_column"), 1, "on both axes")
        text = text_of_tag(tf, STATUS_TAG, viewport=WIN)
        assert "16 cells in 1 band" in text, f"the readout states both: {text!r}"

        # ── (K) scattered cells MERGE into one band ──────────────────
        # The canonical invariant, and the phase this demo did not have until
        # a counterfactual walked straight through it: two rows reached by two
        # separate Ctrl-presses share one column set, so they are ONE band —
        # and the same two cells reached in the other order are the same
        # VALUE. `QItemSelection` is a `QList` that permits overlap, so two Qt
        # selections covering the same cells differ by the order the `select()`
        # calls arrived in, which is why `selectedIndexes()` has to promise
        # de-duplication.
        chord_click(tf, cell(1, 0))
        chord_click(tf, cell(7, 0), ctrl=True)
        forward = cells(tf)
        assert_eq(
            forward,
            [{"rows": [[1, 1], [7, 7]], "columns": [[0, 0]]}],
            "two scattered cells with one column set are ONE band",
        )
        assert_eq(tf.query("/external/band_count"), 1, "and the model says one")
        assert_eq(tf.query("/external/cell_count"), 2, "holding two cells")
        chord_click(tf, cell(7, 0))
        chord_click(tf, cell(1, 0), ctrl=True)
        assert_eq(
            cells(tf),
            forward,
            "the other order is the SAME VALUE — canonical, so `changed` can be "
            "a value comparison at all",
        )

        # ── (G) `All` outlives its count ─────────────────────────────
        # A row header press selects the RECORD, and the wire says `"all"` —
        # no column count in it. Naming the same eight columns explicitly
        # paints identically and is a DIFFERENT VALUE, which is exactly the
        # distinction a Qt range cannot carry.
        chord_click(tf, band_row(4))
        record = cells(tf)
        assert_eq(
            record,
            [{"rows": [[4, 4]], "columns": "all"}],
            "a record is `all` — a statement with no width in it",
        )
        assert_eq(tf.query("/external/selection"), [[4, 4]], "and it IS a selected row")
        assert_eq(tf.query("/external/cell_count"), NCOLS, "extending over all eight columns")
        tf.intervene(
            "/external/cells",
            [{"rows": [[4, 4]], "columns": [[0, NCOLS - 1]]}],
        )
        named = cells(tf)
        assert_eq(
            named,
            [{"rows": [[4, 4]], "columns": [[0, 7]]}],
            "the same eight columns, named",
        )
        assert_eq(
            tf.query("/external/cell_count"),
            NCOLS,
            "the same cells are selected...",
        )
        assert named != record, (
            "...and it is a DIFFERENT VALUE. Add a ninth column and the first "
            "still covers the record while the second does not — the demotion "
            "Qt performs silently"
        )
        assert_eq(
            tf.query("/external/selection"),
            [],
            "which `selectedRows()` reports by not reporting it",
        )

        # ── (H) it reaches assistive technology ──────────────────────
        tf.intervene("/external/cells", [{"rows": [[0, N - 1]], "columns": [[1, 1]]}])
        access = tf.request("scene/access").result
        head = access_node_by_tag(access, f"{TABLE_TAG}_ch1")
        assert head is not None, "the selected column's header is in the AT tree"
        assert_eq(head.get("selected"), True, "and it is aria-selected — Qt has no such state")
        other = access_node_by_tag(access, f"{TABLE_TAG}_ch2")
        assert other is not None, "its neighbour is there too"
        assert_eq(other.get("selected"), False, "and is not")
        marked = [
            n
            for n in access["nodes"]
            if n.get("role") == "gridcell" and n.get("selected") is True
        ]
        assert marked, "the column's windowed cells carry aria-selected"
        rows_marked = [
            n for n in access["nodes"] if n.get("role") == "row" and n.get("selected") is True
        ]
        assert_eq(
            len(rows_marked),
            0,
            "and no row is announced as selected — one of eight columns is not a row",
        )

        # ── (J) the verbs are declared ───────────────────────────────
        schema = tf.query("/external/$schema")
        by_path = {f["path"]: f for f in schema}
        for verb in ("select_cell", "select_column", "toggle_all", "clear"):
            assert verb in by_path, f"$schema must announce the verb {verb!r}"
            assert "channel" in by_path[verb], (
                f"{verb!r} must declare its channel — an undeclared one reads "
                f"as the default, which is `read`, and it is not readable"
            )
            assert_eq(
                by_path[verb]["channel"],
                "invoke",
                f"{verb!r} is a call, not a readable slot",
            )
        for slot in ("cells", "column_selection", "column_count", "behavior"):
            assert slot in by_path, f"$schema must announce the slot {slot!r}"
            # R1504 — the key is present only on the non-default channel, so a
            # readable slot is the ABSENCE of it. Asserted as absence rather
            # than as `"read"`, because that is what the wire says.
            assert "channel" not in by_path[slot], f"{slot!r} is read"


    # ── (I) a grid with no column axis refuses BY NAME ───────────────
    with RpcSubprocess(ROW_SELECT_EXAMPLE, boot_grace=1.5) as row_tf:
        assert_eq(
            row_tf.query("/external/column_count"),
            None,
            "a row-select grid declares NO column axis — null, not zero",
        )
        assert_eq(row_tf.query("/external/behavior"), "rows", "and says a press takes a record")
        assert_eq(
            row_tf.query("/external/section_press"),
            "inert",
            "its headers are the sort control, so they select nothing",
        )
        try:
            row_tf.invoke("/external/select_column", 3)
        except RpcError:
            pass
        else:
            raise AssertionError(
                "selecting a column on a grid that has none must REFUSE — "
                "Qt's selectColumn returns void and the call is simply lost"
            )
        assert_eq(row_tf.query("/external/cells"), [], "and nothing moved")


if __name__ == "__main__":
    run_demo("R1563 §5.27 §5.40 — a selection has two axes", body)
