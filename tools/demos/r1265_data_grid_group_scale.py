#!/usr/bin/env python3
"""R1265 §5.40 — grouped data-grid flatten is O(rows * log groups), not O(rows^2).

Drives `hello-data-grid` via JSON-RPC. R892 grouping flattened the filtered +
sorted rows into group runs by mapping each source row to its group id with
`group_of`, which linear-scanned the label table (O(groups)); `group_rows`
calls that once per source row, and `view` rebuilds the whole flatten every
frame, so grouping by a HIGH-cardinality column (near-unique `Asset` names) was
O(rows * groups) ~ O(rows^2) per paint on the grid's 10k-row target. R1265
precomputes a label -> id index once (the R1261 lesson), making the grouped
flatten O(rows * log groups). The scene output is byte-identical — this demo is
the correctness-at-scale proof (the win itself is invisible to the wire, like
R1261's node-paint scale demo).

  (A) boot: 4 seed rows, ungrouped.
  (B) group by Asset (col 0) at the seed: 4 distinct names => 4 one-member
      groups (the high-cardinality shape at small scale).
  (C) GROW via paste to 40 distinct-Asset rows.
  (D) group by Asset at scale: 40 groups, 80 visible rows, headers and data
      interleaved with no row dropped or duplicated.
  (E) collapse one group / collapse_all / expand_all track exactly.
  (F) ungroup returns to the flat 40-row view.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_query,
)

GRID = "data_grid"
N = 40  # scale row count after the paste-grow (groups == rows, the quadratic case)


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg=None):
    return tf.invoke(f"/external/{verb}", arg)


def cursor(tf, row: int, col: int) -> None:
    tf.intervene("/external/focused_row", row)
    tf.intervene("/external/focused_col", col)


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot: ungrouped ─────────────────────────────────────
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "group"), "none", "boot: ungrouped")
        assert_eq(q(tf, "visible_len"), 4, "flat = 4 data rows")

        # ── (B) group by Asset (col 0): 4 distinct names, one each ──
        assert_eq(inv(tf, "set_group", "0"), 4, "4 distinct Asset names => 4 groups")
        assert_eq(q(tf, "visible_len"), 8, "4 headers + 4 data rows")
        assert_eq(q(tf, "kind_at.0"), "header", "position 0 is a header")
        assert_eq(q(tf, "kind_at.1"), "data", "position 1 is a data row")
        assert_eq(q(tf, "label_at.0"), "Hero", "first-appearance Asset leads")
        assert_eq(q(tf, "source_at.1"), 0, "its one member is source row 0")
        assert_eq(q(tf, "label_at.2"), "Tree", "second group label")
        assert_eq(q(tf, "source_at.3"), 1, "second group's member")

        # ── (C) grow to N distinct-Asset rows via one paste ─────────
        inv(tf, "set_group", None)  # paste writes in flat (source) order
        wait_query(tf, "/external/group", "none", desc="flat view for the paste")
        cursor(tf, 0, 0)  # Asset column, first row
        block = "\n".join(f"G{i}" for i in range(N))  # N distinct names, one column
        assert_eq(inv(tf, "paste", block), N, "all N Asset cells land")
        assert_eq(q(tf, "row_count"), N, f"the grid grew to {N} rows")
        assert_eq(q(tf, "value.0.0"), "G0", "row 0 Asset overwritten")
        assert_eq(q(tf, "value.39.0"), "G39", "the last grown row's Asset")

        # ── (D) group by Asset at scale: N groups, 2N rows, no drop/dup ─
        assert_eq(inv(tf, "set_group", "0"), N, f"{N} distinct Assets => {N} groups")
        assert_eq(q(tf, "visible_len"), 2 * N, f"{N} headers + {N} data, nothing lost")
        # Spot-check the interleave across the range: even = header, odd = data.
        for pos in (0, 1, 2, 3, 2 * N - 2, 2 * N - 1):
            expect = "header" if pos % 2 == 0 else "data"
            assert_eq(q(tf, f"kind_at.{pos}"), expect, f"pos {pos} is a {expect}")
        assert_eq(q(tf, "label_at.0"), "G0", "first header label (source order)")
        assert_eq(q(tf, "source_at.1"), 0, "first data row source")
        assert_eq(q(tf, "label_at.20"), "G10", "a mid-range header label")
        assert_eq(q(tf, "source_at.21"), 10, "the mid-range group's member")
        assert_eq(q(tf, f"label_at.{2 * N - 2}"), f"G{N - 1}", "the last header label")
        assert_eq(q(tf, f"source_at.{2 * N - 1}"), N - 1, "the last data row source")
        assert_eq(q(tf, "source_at.0"), None, "a header reports a Null source")

        # ── (E) collapse tracks exactly at scale ────────────────────
        assert_eq(inv(tf, "toggle_group", 0), True, "collapse the first group")
        wait_query(tf, "/external/collapsed.0", True, desc="group 0 collapsed")
        assert_eq(q(tf, "visible_len"), 2 * N - 1, "collapsing one 1-member group hides one row")
        inv(tf, "collapse_all", None)
        wait_query(tf, "/external/visible_len", N, desc="collapse_all leaves only the N headers")
        inv(tf, "expand_all", None)
        wait_query(tf, "/external/visible_len", 2 * N, desc="expand_all restores all members")

        # ── (F) ungroup returns to the flat scaled view ─────────────
        inv(tf, "set_group", None)
        wait_query(tf, "/external/group", "none", desc="ungrouped")
        assert_eq(q(tf, "group_count"), 0, "no groups when flat")
        assert_eq(q(tf, "visible_len"), N, f"flat view shows all {N} rows")


if __name__ == "__main__":
    sys.exit(run_demo("R1265 §5.40 — grouped data-grid flatten scales (O(rows log groups))", body))
