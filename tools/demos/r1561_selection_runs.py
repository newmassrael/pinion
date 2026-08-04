#!/usr/bin/env python3
"""R1561 §5.27 §5.40 — a selection is a set of RUNS, not a set of rows.

Drives the `hello-multi-select` binding (10 000-row virtualized list, Qt
`QItemSelectionModel` `ExtendedSelection` analogue) over JSON-RPC.

A selection is not a set of rows. It is a set of *runs*: "rows 0 through
9 999" is one fact, and holding it as ten thousand facts makes the cost of
storing it, comparing it, and — on a framework whose §2 #7 invariant is that
the scene is queryable as data — **saying** it proportional to the model
rather than to the statement. Measured on this very binding before the model
changed, and again after, same box, same probe, same steady state:

    invoke("select_all")   10 000 integers  58 890 bytes  15.8 ms
                        ->  one run             11 bytes   1.0 ms
    query("selection")     10 000 integers  58 890 bytes   1.2 ms
                        ->  one run             11 bytes   0.3 ms

The decisive witness is a **bound on the payload** rather than a clock: a
whole-model selection answers in under 64 bytes, which no per-row encoding
can do at any speed. The negative control is right beside it — a scattered
selection really does grow the run count, so the number being asserted is a
measurement of the representation and not a constant.

  (A) boot — nothing selected; the empty set is `[]`, count 0.
  (B) a span is ONE run — Shift+End across the model, payload bounded.
  (C) Ctrl+A is one run, and `selection_count` is exact.
  (D) a hole SPLITS the run and closing it MERGES back — the negative
      control, and canonicality in both directions.
  (E) the count is a function of the selection, not of the wire size.
  (F) canonical however built — runs sent out of order / overlapping /
      abutting all reach the same value.
  (G) a run straddling `item_count` is TRIMMED to the last row, not dropped.
  (H) a malformed payload is REFUSED whole; the selection is untouched.
  (I) `selection_count` is derived, so it is read-only.
  (J) the screen says it — the status bar reads `selected N rows in K runs`,
      the run count on the glass beside the row count.

Against Qt 6.11: `QItemSelectionModel` has `hasSelection()` and no count
accessor, so counting means `selectedRows().size()` — one `QModelIndex` per
selected row, built to read a length; `selectedIndexes()` is documented to
return a list that "contains no duplicates, and is not sorted", so the first
selected row costs a scan and equal selections need not compare equal; and
only `QItemSelection::merge` promises non-overlapping ranges, so what
`selection()` holds is a function of the call history rather than of the
selection. Phases (D), (E) and (F) are those three differences, read over the
wire.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    abs_rects_of,
    find_by_tag,
    indexed_tags,
    run_demo,
    runs_of,
    selection_rows,
    texts_of,
    wait_query,
    wait_until,
)

EXAMPLE = "hello-multi-select"
WIN = (380, 480)
N = 10_000
LIST_TAG = "vlist"
STATUS_TAG = "vlist_status"

#: What a whole-model selection may cost on the wire. `[[0, 9999]]` is 11
#: bytes; the pre-R1561 answer was 58 890. Anything between the two is a
#: representation that still scales with the model, so the bound is set close
#: to the fact's own size rather than merely below the old number.
MAX_SELECTION_BYTES = 64


def mod_key(tf, name: str, *, shift: bool = False, ctrl: bool = False) -> None:
    """One `scene/key` with held modifiers, then release — the R763
    `scene/modifiers` press/release pair the harness documents."""
    tf.modifiers(shift=shift, ctrl=ctrl)
    tf.key(path=LIST_TAG, name=name)
    tf.modifiers()


def selection(tf) -> list[int]:
    """The selected ROWS, ascending — through the shared decoder."""
    return selection_rows(tf.query("/external/selection"))


def raw(tf) -> list:
    """The `"selection"` answer as it arrives: the runs themselves.

    Assertions about the *representation* read this. Assertions about which
    rows are selected read `selection`.
    """
    return tf.query("/external/selection")


def wire_bytes(value) -> int:
    """How many bytes the answer occupies as JSON — the cost an agent pays to
    ask, and the number this round exists to bound."""
    return len(json.dumps(value, separators=(",", ":")))


def status_text(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "the status bar must be in the paint tree"
    return texts_of(node)[0]


def refused(fn) -> bool:
    """Whether `fn()` was refused by the endpoint."""
    try:
        fn()
    except RpcError:
        return True
    return False


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # The `path=` key form walks the PAINT tree for the tag, so the first
        # snapshot is what makes the list addressable (r780's boot shape).
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert find_by_tag(snap, LIST_TAG) is not None, "list container present at boot"

        # ── (A) boot ────────────────────────────────────────────────
        assert_eq(tf.query("/external/mode"), "multi", "a multi-select model")
        assert_eq(tf.query("/external/item_count"), N, "over the whole dataset")
        assert_eq(raw(tf), [], "nothing selected is the empty run list")
        assert_eq(tf.query("/external/selection_count"), 0, "and a count of zero")
        assert_eq(selection(tf), [], "which decodes to no rows")

        # ── (B) a span is ONE run ───────────────────────────────────
        tf.request("focus/set", {"tag": LIST_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == LIST_TAG,
            desc="list focused for keyboard nav",
        )
        tf.key(path=LIST_TAG, name="ArrowDown")  # from nothing -> row 0
        wait_query(tf, "/external/selection", runs_of([0]),
                   desc="the first ArrowDown selects just row 0")
        mod_key(tf, "End", shift=True)
        wait_query(tf, "/external/selection", [[0, N - 1]],
                   desc="Shift+End extends across the model — as one run")
        span = raw(tf)
        assert_eq(len(span), 1, "a contiguous span is a single run")
        assert_eq(span[0], [0, N - 1], "and the run names its two ends")
        assert wire_bytes(span) <= MAX_SELECTION_BYTES, (
            f"a whole-model selection must cost at most {MAX_SELECTION_BYTES} "
            f"bytes on the wire, got {wire_bytes(span)}"
        )
        assert_eq(tf.query("/external/selection_count"), N,
                  "every row is selected, and the count says so")
        assert_eq(len(selection(tf)), N, "the runs really do name every row")

        # ── (C) Ctrl+A is the same one run ──────────────────────────
        tf.invoke("/external/clear", None)
        assert_eq(raw(tf), [], "cleared")
        assert_eq(tf.invoke("/external/select_all", None), [[0, N - 1]],
                  "Ctrl+A answers with the one run every row forms")
        assert wire_bytes(raw(tf)) <= MAX_SELECTION_BYTES, "select-all is bounded too"
        assert_eq(tf.query("/external/selection_count"), N, "and counts every row")

        # ── (D) a hole splits; closing it merges back ───────────────
        # THE NEGATIVE CONTROL: if `run_count` were a constant, or the runs
        # were never merged, one of these two would fail.
        tf.invoke("/external/toggle", 5_000)
        after_hole = raw(tf)
        assert_eq(len(after_hole), 2, "punching a hole makes two runs")
        assert_eq(after_hole, [[0, 4_999], [5_001, N - 1]], "split at the hole")
        assert_eq(tf.query("/external/selection_count"), N - 1, "one row fewer")
        assert 5_000 not in selection(tf), "and that row is not selected"
        tf.invoke("/external/toggle", 5_000)
        assert_eq(raw(tf), [[0, N - 1]],
                  "putting the row back MERGES the two runs — canonical both ways")
        assert_eq(tf.query("/external/selection_count"), N, "and the count returns")

        # ── (E) the count is a function of the selection ────────────
        tf.invoke("/external/clear", None)
        for row in (10, 20, 30):
            tf.invoke("/external/toggle", row)
        scattered = raw(tf)
        assert_eq(len(scattered), 3, "three apart rows really are three runs")
        assert_eq(tf.query("/external/selection_count"), 3, "and three rows")
        assert_eq(selection(tf), [10, 20, 30], "which rows they are")
        # Now the same COUNT from a single run, to show the count does not
        # track the representation's size.
        tf.invoke("/external/clear", None)
        tf.invoke("/external/select", 40)
        tf.invoke("/external/extend_to", 42)
        assert_eq(len(raw(tf)), 1, "three adjacent rows are ONE run")
        assert_eq(tf.query("/external/selection_count"), 3,
                  "the same three rows — the count is rows, the runs are storage")

        # ── (F) canonical however it was built ──────────────────────
        # Out of order, overlapping, and abutting: every spelling of
        # "rows 100 through 199" reaches the same value. Qt's `QItemSelection`
        # keeps what it was given (only `merge` promises non-overlap), so the
        # equivalent there depends on the order the calls arrived in.
        canonical = [[100, 199]]
        for spelling in (
            [[100, 199]],
            [[150, 199], [100, 149]],
            [[100, 160], [140, 199]],
            [[100, 120], [121, 199], [110, 130]],
        ):
            tf.intervene("/external/selection", spelling)
            assert_eq(raw(tf), canonical, f"{spelling} is rows 100..199")
        assert_eq(tf.query("/external/selection_count"), 100, "one hundred rows")
        assert_eq(tf.query("/external/anchor"), 199,
                  "the anchor follows the greatest restored row")

        # ── (G) a straddling run is TRIMMED, not dropped ────────────
        tf.intervene("/external/selection", [[N - 5, N + 500]])
        assert_eq(raw(tf), [[N - 5, N - 1]],
                  "the part of the run the model has is kept; the rest is dropped")
        assert_eq(tf.query("/external/selection_count"), 5, "five real rows")
        tf.intervene("/external/selection", [[N, N + 10]])
        assert_eq(raw(tf), [], "a run entirely past the model selects nothing")

        # ── (H) a malformed payload is refused whole ────────────────
        tf.intervene("/external/selection", [[7, 9]])
        before = raw(tf)
        assert refused(lambda: tf.intervene("/external/selection", [1, 2, 3])), (
            "an array of bare indices is not an array of runs"
        )
        assert_eq(raw(tf), before, "a refused write leaves the selection alone")
        assert refused(lambda: tf.intervene("/external/selection", [[1, "x"]])), (
            "a run whose end is not a number is refused"
        )
        assert_eq(raw(tf), before, "again, untouched")

        # ── (I) the count is derived, so it is read-only ────────────
        assert refused(lambda: tf.intervene("/external/selection_count", 3)), (
            "the count is not a second way to set the selection"
        )
        assert_eq(tf.query("/external/selection_count"), 3, "still three rows")

        # ── (J) the screen says it ──────────────────────────────────
        tf.invoke("/external/select_all", None)
        wait_until(
            lambda: "10000 rows in 1 run" in status_text(tf) or None,
            desc="the status bar reads the row count AND the run count",
        )
        assert "in 1 run" in status_text(tf), "one run on the glass"
        tf.invoke("/external/toggle", 5_000)
        wait_until(
            lambda: "9999 rows in 2 runs" in status_text(tf) or None,
            desc="punching a hole shows two runs on the glass",
        )
        # And the windowing still holds while 9 999 rows are selected: the
        # paint asks the run set per RENDERED row, so the selection's size is
        # not the paint's size. This is the axis's own claim — per-frame work
        # bounded by what is visible — now true of the selection too.
        snap = tf.snapshot(source="paint", viewport=WIN)
        rendered = indexed_tags(abs_rects_of(snap), f"{LIST_TAG}#")
        assert rendered, "the window really rendered rows"
        assert len(rendered) < 30, (
            f"9 999 selected rows must not materialize: {len(rendered)} of {N} rendered"
        )
        assert all(r in selection(tf) or r == 5_000 for r in rendered), (
            "every rendered row except the toggled hole is inside the selection"
        )


if __name__ == "__main__":
    run_demo("R1561 §5.27 §5.40 — a selection is a set of runs", body)
