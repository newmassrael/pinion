#!/usr/bin/env python3
"""R1222 §5.38 §5.22 — cell-range COPY: the selected rectangle as TSV.

R953 gave `hello-cell-select` a spreadsheet cell-range selection (anchor +
extent rectangle) but explicitly deferred "range copy". R1222 adds it on the
shared `Table` coordinator: `query cell_selection_tsv` serializes the selected
rectangle as TSV (tab columns, newline rows) — the spreadsheet / Sheets clipboard
form. This is the AI-first "copy the selection" read (§2 #2): an AI copies a
range by reading it, no pixels and no platform clipboard. The human peer is
Ctrl+C, which writes the SAME TSV to the system clipboard — that write is
HW-gated (driving it in an automated test would clobber / race a live display's
clipboard), so this demo asserts only the deterministic TSV read.

Every grid built on `Table` gains `cell_selection_tsv` (additive, no behavior
change to existing paths). The dataset is the R953 6x4 numeric spreadsheet:

    row0: 120 135 128 150      row3:  45  50  48  52
    row1:  88  92  99 105      row4: 310 325 330 340
    row2: 210 198 205 220      row5:  17  19  22  25

  (A) boot: no selection -> the TSV is Null.
  (B) a single cell -> a single TSV token.
  (C) a grown rectangle -> row-major TSV mirroring the painted selection.
  (D) a full row / a full column / the whole grid.
  (E) the TSV re-derives from `cell_selection` (the rectangle) + the data, so
      it never disagrees with the painted bounds.
  (F) clearing the range drops the TSV back to Null.

Run from the workspace root:
    cargo build -p hello-cell-select --release
    python3 tools/demos/r1222_cell_range_copy.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

T = "grid"


def q(d: RpcSubprocess, slot: str) -> Any:
    return d.query(f"/external/{slot}")


def select(d: RpcSubprocess, spec: str) -> None:
    assert d.invoke("/external/select-cell", spec) is True, f"select-cell {spec}"


def extend(d: RpcSubprocess, spec: str) -> None:
    assert d.invoke("/external/extend-cell", spec) is True, f"extend-cell {spec}"


def tsv(d: RpcSubprocess) -> Any:
    return q(d, "cell_selection_tsv")


def body() -> None:
    with RpcSubprocess("hello-cell-select") as d:
        # ── (A) boot: no selection -> Null TSV ───────────────────────
        wait_until(lambda: True if q(d, "rows") == 6 else None, desc="grid ready")
        assert_eq(q(d, "cols"), 4, "4 columns")
        assert_eq(q(d, "cell_selection"), None, "no selection at boot")
        assert_eq(tsv(d), None, "no selection -> the TSV read is Null")

        # ── (B) a single cell -> a single token ──────────────────────
        select(d, "1,1")
        assert_eq(q(d, "cell_selection"), "1,1,1,1", "single-cell rectangle")
        assert_eq(q(d, "cell_selection_count"), 1, "one cell")
        assert_eq(tsv(d), "92", "the lone selected cell value")

        # ── (C) a grown rectangle -> row-major TSV ───────────────────
        extend(d, "2,2")  # bbox (1,1)-(2,2)
        assert_eq(q(d, "cell_selection"), "1,1,2,2", "the rectangle grew from the anchor")
        assert_eq(q(d, "cell_selection_count"), 4, "2x2 = 4 cells")
        assert_eq(tsv(d), "92\t99\n198\t205", "row-major, tab columns, newline rows")

        # ── (D) a full row, a full column, the whole grid ────────────
        select(d, "0,0")
        extend(d, "0,3")
        assert_eq(tsv(d), "120\t135\t128\t150", "the whole first row")
        select(d, "0,3")
        extend(d, "5,3")
        assert_eq(tsv(d), "150\n105\n220\n52\n340\n25", "the whole last column")
        select(d, "0,0")
        extend(d, "5,3")
        assert_eq(q(d, "cell_selection_count"), 24, "6x4 = the whole grid")
        assert_eq(
            tsv(d),
            "120\t135\t128\t150\n88\t92\t99\t105\n210\t198\t205\t220\n"
            "45\t50\t48\t52\n310\t325\t330\t340\n17\t19\t22\t25",
            "the whole spreadsheet as one TSV block",
        )

        # ── (E) the TSV re-derives from the rectangle + data ─────────
        # A sub-rectangle in the lower-right; the TSV must match the bounds the
        # `cell_selection` wire reports (one source of truth, no disagreement).
        select(d, "3,1")
        extend(d, "4,2")
        assert_eq(q(d, "cell_selection"), "3,1,4,2", "lower-right 2x2")
        assert_eq(tsv(d), "50\t48\n325\t330", "TSV mirrors exactly those bounds")
        # A single edge cell.
        select(d, "5,0")
        assert_eq(q(d, "cell_selection_count"), 1, "one cell again")
        assert_eq(tsv(d), "17", "the bottom-left corner value")

        # ── (F) clearing the range drops the TSV ─────────────────────
        assert d.invoke("/external/clear-cell-selection", None) is True, "clear handled"
        assert_eq(q(d, "cell_selection"), None, "selection cleared")
        assert_eq(q(d, "cell_selection_count"), 0, "zero cells")
        assert_eq(tsv(d), None, "cleared -> the TSV is Null again")
        # Re-selecting after a clear works (the anchor re-arms).
        select(d, "2,0")
        extend(d, "2,3")
        assert_eq(tsv(d), "210\t198\t205\t220", "the whole third row after re-selecting")


if __name__ == "__main__":
    sys.exit(run_demo("R1222 cell-range copy (TSV)", body))
