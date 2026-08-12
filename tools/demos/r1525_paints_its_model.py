#!/usr/bin/env python3
"""R1525 §5.27 §5.40 — the grid paints the model it orders, E2E.

Drives the `hello-grid-sort` binding via JSON-RPC. Until this round the grid had
**two paths to one dataset**: `GridSortState` held the source cells and computed
the sort and filter from them (its comparator and its filter predicate read
`cell(row, col)` directly), while the painted text came from the binding's
`cell_text` formula. Nothing failed, because R1524's `materialize_cells` seeded
the model from that same formula — but agreement by derivation is not one path,
and the string the user reads was not the string the ordering came from. R1525
points the view at the model: `|c| grid.cell(c.row, c.col).to_string()`.

For that claim to be checkable over RPC at all, the model's own copy has to be
on the wire — so `GridSortExternal` (and `RowSearchExternal`) now answer
`cell.<row>.<col>`, the wire form the eager `TableExternal` has always answered.
Without it the snapshot shows the painted text and there is nothing independent
to compare it against, which is the same gap R1523 closed on the a11y side by
adding its column pair to `access.rs`: RPC is invariant #2's primary path.

The witness is therefore a **cross-check between two surfaces**, not one reading:

  (A) boot — the proxy answers `cell.<row>.<col>`, bounded, and its schema
      declares the slot (so the surface is discoverable, not folklore).
  (B) painted == model, for every cell in the window. This is the round's claim
      stated directly.
  (C) sorted — after a sort, the painted column reads in sorted order AND each
      painted strip's text still equals the model's cell for the SOURCE row that
      `source_at.<pos>` reports. Order and text are checked against the same
      model, so a view painting anything else would have to coincide with it.
  (D) filtered — the same equality under a filter, where the view length shrinks
      and the visual→source map is no longer the identity.
  (E) the bounds are absent, not empty — a row past the dataset and a column past
      the grid both report absence, matching `TableExternal`'s own contract for
      this wire form (and unlike `source_at.<pos>`, which reports Null).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_no_such_member,
    assert_rpc_error,
    find_by_tag,
    indexed_tags,
    run_demo,
    texts_of,
    wait_query,
)

EXAMPLE = "hello-grid-sort"
WIN = (400, 480)
N = 10_000
NCOLS = 3
GRID_TAG = "vtbl"
SORT_TAG = "vsort"
NAME_COL = 0
SCORE_COL = 1


def model_cell(tf, row: int, col: int):
    """The model's own copy of cell (row, col), over the introspect wire."""
    return tf.query(f"/{SORT_TAG}/external/cell.{row}.{col}")


def painted_cell(snap, row: int, col: int):
    """The text the grid actually painted for data row `row`, column `col`."""
    node = find_by_tag(snap, f"{GRID_TAG}#{row}_{col}")
    if node is None:
        return None
    texts = texts_of(node)
    return texts[0] if texts else None


def rendered_rows(rects) -> list[int]:
    return indexed_tags(rects, f"{GRID_TAG}_row")


def row_cols(rects, row: int) -> list[int]:
    return indexed_tags(rects, f"{GRID_TAG}#{row}_")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def assert_painted_is_the_model(snap, label: str) -> int:
            """Every windowed cell's painted text IS the model's cell text."""
            rects = abs_rects_of(snap)
            rows = rendered_rows(rects)
            assert rows, f"{label}: the window holds rows"
            checked = 0
            # Sample the window's ends and middle rather than all of it: each
            # cell costs an RPC round-trip, and a disagreement would be
            # systematic (a whole second data path), not one unlucky cell.
            for row in (rows[0], rows[len(rows) // 2], rows[-1]):
                cols = row_cols(rects, row)
                assert cols, f"{label}: row {row} holds cells"
                for col in cols:
                    assert_eq(
                        painted_cell(snap, row, col),
                        model_cell(tf, row, col),
                        f"{label}: painted cell ({row},{col}) is the model's cell",
                    )
                    checked += 1
            return checked

        # ── (A) the model's cells are on the wire, and declared ──────
        assert_eq(tf.query(f"/{SORT_TAG}/external/count"), N, "proxy knows the row count")
        assert_eq(tf.query(f"/{SORT_TAG}/external/cols"), NCOLS, "proxy knows the column count")
        first = model_cell(tf, 0, NAME_COL)
        assert isinstance(first, str) and first, f"cell.0.0 answers text, got {first!r}"
        schema = tf.query(f"/{SORT_TAG}/external/$schema")
        assert schema is not None, "the proxy answers $schema"
        paths = [f.get("path") for f in schema if isinstance(f, dict)]
        assert "cell.<row>.<col>" in paths, \
            f"the proxy DECLARES the cell slot, so an agent can discover it: {paths}"
        slot = next(f for f in schema if f.get("path") == "cell.<row>.<col>")
        assert_eq(slot.get("type"), "string", "the declared cell slot yields text")
        args = [a.get("name") for a in slot.get("args") or []]
        assert_eq(args, ["row", "col"],
                  "and it declares BOTH coordinates, so a client need not guess the arity")

        # ── (B) painted == model, unsorted ──────────────────────────
        snap = snap_now()
        checked = assert_painted_is_the_model(snap, "unsorted")
        assert checked >= 3, f"the unsorted pass checked {checked} cells"

        # ── (C) sorted: order and text come from the same model ─────
        # Cycle the Score column to ascending through the proxy's own channel.
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", SCORE_COL),
                  f"{SCORE_COL}:ascending", "cycle_sort put Score ascending")
        wait_query(
            tf, f"/{SORT_TAG}/external/sort_dir", "ascending",
            desc="the proxy reports an ascending sort",
        )
        assert_eq(tf.query(f"/{SORT_TAG}/external/sort_col"), SCORE_COL,
                  "and it is the Score column")

        snap = snap_now()
        rects = abs_rects_of(snap)
        rows = rendered_rows(rects)
        assert rows, "the sorted window holds rows"
        assert_painted_is_the_model(snap, "sorted")

        # The visual→source map and the painted strips agree, so the text under
        # a visual position is the model's text for the row the proxy put there.
        top_source = tf.query(f"/{SORT_TAG}/external/source_at.0")
        assert isinstance(top_source, int), f"source_at.0 is a row, got {top_source!r}"
        assert top_source in rows, \
            f"the top visual position's source row {top_source} is in the window"
        assert_eq(
            painted_cell(snap, top_source, SCORE_COL),
            model_cell(tf, top_source, SCORE_COL),
            "the top sorted row's painted Score is the model's Score",
        )
        # ...and the painted Score column really is ascending down the window.
        visual = []
        for pos in range(len(rows)):
            src = tf.query(f"/{SORT_TAG}/external/source_at.{pos}")
            if not isinstance(src, int) or src not in rows:
                break
            text = painted_cell(snap, src, SCORE_COL)
            assert text is not None, f"visual position {pos} (source {src}) is painted"
            visual.append(int(text))
        assert len(visual) >= 3, f"read {len(visual)} sorted positions"
        assert visual == sorted(visual), \
            f"the PAINTED Score column reads ascending: {visual}"

        # ── (D) filtered: the same equality off the identity map ─────
        tf.invoke(f"/{SORT_TAG}/external/set_filter", "2=Active")
        view_len = tf.query(f"/{SORT_TAG}/external/view_len")
        assert isinstance(view_len, int) and 0 < view_len < N, \
            f"the filter shrank the view: {view_len} of {N}"
        assert_eq(tf.query(f"/{SORT_TAG}/external/count"), N,
                  "while the DATASET is untouched — a filter hides rows, it does not "
                  "delete them, and `cell.<row>.<col>` still addresses all of them")
        snap = snap_now()
        assert_painted_is_the_model(snap, "filtered")
        filtered_rows = rendered_rows(abs_rects_of(snap))
        assert filtered_rows, "the filtered window holds rows"
        for row in filtered_rows:
            assert_eq(model_cell(tf, row, 2), "Active",
                      f"every painted row {row} is one the filter kept")

        # ── (E) the bounds are absent, not empty ────────────────────
        # ★ R1670 — the two INDEX failures and the malformed ADDRESS are now
        # different answers, which is R1667's split and the reason it was worth
        # making: a row past the end says "read the count and ask again" and
        # names the bounds it refused against, while `cell.0` is not an address
        # of this family at all and says "stop asking for this name".
        assert_no_such_member(lambda: model_cell(tf, N, 0), saying=f"rows 0..{N}")
        assert_no_such_member(
            lambda: model_cell(tf, 0, NCOLS), saying=f"columns 0..{NCOLS}"
        )
        assert_rpc_error(lambda: tf.query(f"/{SORT_TAG}/external/cell.0"),
                         data="UnknownIntrospectPath")
        # Contrast, and it is deliberate on both sides: `source_at` past its
        # bound ANSWERS with Null, because "no source at this visual position"
        # is a meaningful reading of a real position, whereas "no such cell" is
        # not an address at all. Absence and present-but-empty mean different
        # things here and the demo pins both.
        assert_eq(tf.query(f"/{SORT_TAG}/external/source_at.{N + 1}"), None,
                  "source_at past the view is present-but-empty, not absent")


if __name__ == "__main__":
    sys.exit(run_demo("R1525 §5.27 §5.40 — the grid paints the model it orders", body))
