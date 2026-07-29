#!/usr/bin/env python3
"""R846 §5.27 §5.40 — sortable grouped data grid (the asset table) E2E.

Drives `hello-grouped-grid-sort` via JSON-RPC. Ties the group-by work into the
production asset table: a clickable column header sorts 10,000 rows through a
GridSortState (R778), whose sorted order feeds GroupOrderState (the 2nd consumer
of the R844 order-source — a *grid* sort proxy where R844 used a 1-D list one),
so rows sort WITHIN their asset-type groups. Four coordinators:

  * primary `VirtualSelectExternal` at `ggrid` — row selection by source index;
  * extra `GridSortExternal` at `gsort` — column-sort proxy (`gsort#h<col>`);
  * extra `GroupOrderExternal` at `ggrp` — groups the sorted order;
  * extra `ScrollBarExternal`.

The witness (§2 #7 scene-as-data): sorting a column re-derives the grouped
flatten over the new order (the group headers reorder, the members within each
group reorder); group collapse + row selection coexist and survive the sort.

  (A) boot — sortable column headers, unsorted source order, visible_len 10006.
  (B) sort Name descending — the highest-source group leads, source 9999 is the
      first member (the grid sort feeds the grouping — pointer-keyed memo
      recomputed); visible_len unchanged.
  (C) select-by-row — click row 9999 (now at the top); selected == 9999.
  (D) collapse the leading group — visible_len drops by its count; the row
      leaves the window; selection held.
  (E) unsort — group order returns to source order.
  (F) sort Size ascending — a different column sorts; grouping intact.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_query,
)

EXAMPLE = "hello-grouped-grid-sort"
WIN = (400, 540)
N = 10_000
NGROUPS = 6
NCOLS = 3
VISIBLE_BOOT = N + NGROUPS  # 10006
MEMBERS = [1667, 1667, 1667, 1667, 1666, 1666]
GRID_TAG = "ggrid"
GROUP_TAG = "ggrp"
SORT_TAG = "gsort"
SCROLL_TAG = "ggrid_scroll"


def data_sources(snap) -> list[int]:
    return sorted(
        int(t.split("#", 1)[1]) for t in abs_rects_of(snap) if t.startswith(f"{GRID_TAG}#")
    )


def gg(d, path):
    return d.query(f"/{GROUP_TAG}/external/{path}")


def gs(d, path):
    return d.query(f"/{SORT_TAG}/external/{path}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot ─────────────────────────────────────────────────────
        for c in range(NCOLS):
            assert f"{SORT_TAG}#h{c}" in rects, f"sortable column header {c} present"
        assert f"{GROUP_TAG}#0" in rects, "Mesh group header present"
        assert_eq(gs(tf, "sort"), "none", "boot is unsorted")
        assert_eq(gs(tf, "cols"), NCOLS, "grid has 3 columns")
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "6 headers + 10000 rows")
        assert_eq(gg(tf, "count"), N, "group proxy knows source count")
        assert_eq(gg(tf, "group_count"), NGROUPS, "six groups")
        assert_eq(gg(tf, "member_count_at.0"), MEMBERS[0], "Mesh member count")
        assert_eq(gg(tf, "collapsed.0"), False, "group 0 boots expanded")
        assert_eq(gg(tf, "kind_at.0"), "header", "pos 0 is a header")
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 (Mesh) leads source order")
        assert_eq(gg(tf, "source_at.1"), 0, "pos 1 is Mesh's first member (source 0)")
        srcs = data_sources(snap)
        assert srcs[:3] == [0, 6, 12], f"unsorted Mesh members in source order: {srcs[:3]}"

        # ── (B) sort Name descending: the grid sort feeds the grouping ───
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 0), "0:ascending", "cycle col 0 -> asc")
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 0), "0:descending", "cycle col 0 -> desc")
        assert_eq(gs(tf, "sort"), "0:descending", "Name column sorted descending")
        assert_eq(gs(tf, "sort_col"), 0, "active sort column is 0 (Name)")
        assert_eq(gs(tf, "sort_dir"), "descending", "active sort direction is descending")
        # Source 9999 (group 9999 % 6 == 3) leads the descending flatten — the
        # grouping re-derived over the grid's new order (R844 pointer memo).
        wait_query(tf, f"/{GROUP_TAG}/external/group_at.0", 3,
                   desc="highest source's group (3) leads after Name-desc")
        assert_eq(gg(tf, "source_at.1"), 9999, "source 9999 is the first descending member")
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "sorting does not change visible_len")

        # ── (C) select the now-leading row ───────────────────────────────
        tf.click(path=f"{GRID_TAG}#9999")
        wait_query(tf, "/external/selected", 9999, desc="click selected row source 9999")

        # ── (D) collapse the leading group: hide its rows, keep selection ─
        shrunk = VISIBLE_BOOT - MEMBERS[3]  # group 3 = 1667 -> 8339
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 3), shrunk, "collapse group 3 -> 8339")
        assert_eq(gg(tf, "visible_len"), shrunk, "visible_len reflects the collapse")
        assert_eq(gg(tf, "collapsed.3"), True, "group 3 collapsed")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 9999 not in data_sources(snap), "collapsed group 3 hides row 9999"
        assert_eq(tf.query("/external/selected"), 9999, "row selection survives the collapse")

        # ── (E) unsort: group order returns to source order ──────────────
        tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 3)  # re-expand
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 0), "none", "cycle col 0 -> unsorted")
        assert_eq(gs(tf, "sort"), "none", "unsorted again")
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 leads source order again")
        assert_eq(gg(tf, "source_at.1"), 0, "pos 1 is source 0 again")

        # ── (F) sort a different column ──────────────────────────────────
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1), "1:ascending", "cycle col 1 (Size) -> asc")
        assert_eq(gs(tf, "sort"), "1:ascending", "Size column sorted ascending")
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "grouping intact under the new sort")
        assert_eq(gg(tf, "group_count"), NGROUPS, "still six groups")


if __name__ == "__main__":
    sys.exit(run_demo("R846 §5.27 §5.40 — sortable grouped data grid", body))
