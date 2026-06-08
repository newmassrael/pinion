#!/usr/bin/env python3
"""R854 §5.27 §5.12 — per-group aggregates (count + total Size).

Drives hello-grouped-grid-sort over JSON-RPC. R843-R846 grouped 10,000 asset
rows but the headers showed only a member count. R854 adds the aggregate every
asset browser shows ("Textures: 1,234 items, 5.6 MB"): each group's total Size,
displayed in its (collapse-surviving) header and exposed AI-first through a
query-only `QueryOnlyIntrospect` external at `/ggs_agg/external`.

  (A) the aggregate surface reports group_count / total_count / total_size and
      per-group count + Size; the counts agree with the group proxy's own.
  (B) the leading group's header is present (where the aggregate is displayed).
  (C) the aggregate is independent of the column sort (it's over membership).
  (D) the aggregate survives a collapse — it rides the always-shown header.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-grouped-grid-sort"
WIN = (400, 540)
N = 10_000
NGROUPS = 6
AGG = "/ggs_agg/external"
GROUP_TAG = "ggrp"
SORT_TAG = "gsort"


def row_size(i: int) -> int:
    return (i * 13) % 990 + 8


# Expected per-group aggregates (the row data is a pure function of the index).
COUNTS = [0] * NGROUPS
SIZES = [0] * NGROUPS
for _i in range(N):
    _g = _i % NGROUPS
    COUNTS[_g] += 1
    SIZES[_g] += row_size(_i)
TOTAL_SIZE = sum(SIZES)


def agg(tf, path):
    return tf.query(f"{AGG}/{path}")


def gg(tf, path):
    return tf.query(f"/{GROUP_TAG}/external/{path}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the aggregate query surface ──────────────────────────
        assert_eq(agg(tf, "group_count"), NGROUPS, "six groups")
        assert_eq(agg(tf, "total_count"), N, "total row count")
        assert_eq(agg(tf, "total_size"), TOTAL_SIZE, "total Size across all groups")
        for g in range(NGROUPS):
            assert_eq(agg(tf, f"group.{g}.count"), COUNTS[g], f"group {g} member count")
            assert_eq(agg(tf, f"group.{g}.size"), SIZES[g], f"group {g} total Size")
        # The per-group counts sum to the total count, and sizes to the total.
        assert_eq(sum(agg(tf, f"group.{g}.count") for g in range(NGROUPS)), N, "counts sum to N")
        assert_eq(sum(agg(tf, f"group.{g}.size") for g in range(NGROUPS)), TOTAL_SIZE,
                  "per-group sizes sum to the reported total")
        # Out-of-range group + unknown path are rejected (None -> RPC error).
        for bad in ("group.99.count", "group.0.bogus", "nonsense"):
            rejected = False
            try:
                agg(tf, bad)
            except RpcError:
                rejected = True
            assert rejected, f"{bad!r} is not a declared aggregate path"

        # ── (B) the leading group header (where it's displayed) ──────
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
        assert f"{GROUP_TAG}#0" in rects, "the leading group header is painted"

        # ── (C) aggregate is independent of the column sort ──────────
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # Size ascending
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # Size descending
        assert_eq(agg(tf, "total_size"), TOTAL_SIZE, "total unchanged by the sort")
        assert_eq(agg(tf, "group.0.size"), SIZES[0], "group 0 Size unchanged by the sort")
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # back to unsorted

        # ── (D) aggregate survives a collapse (rides the header) ─────
        assert_eq(gg(tf, "collapsed.0"), False, "group 0 boots expanded")
        tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0)  # collapse group 0
        assert_eq(gg(tf, "collapsed.0"), True, "group 0 collapsed")
        assert_eq(agg(tf, "group.0.size"), SIZES[0], "the Size aggregate survives the collapse")
        assert_eq(agg(tf, "group.0.count"), COUNTS[0], "the count survives the collapse")
        rects2 = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
        assert f"{GROUP_TAG}#0" in rects2, "the collapsed group header still shows (aggregate stays visible)"
        tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0)  # re-expand


if __name__ == "__main__":
    sys.exit(run_demo("R854 §5.27 §5.12 — per-group aggregates (count + total Size)", body))
