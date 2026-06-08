#!/usr/bin/env python3
"""R845 §5.27 §5.40 — grouped multi-column data grid E2E.

Drives `hello-grouped-grid` via JSON-RPC. The R843 `group_rows` flatten's
SECOND structural consumer: a 3-column grid (Name / Size / Modified) grouped by
asset type, with collapsible group-header rows spanning the columns. Three
coordinators:

  * primary `VirtualSelectExternal` at `ggrid` — ROW selection by source index
    (each data row is the click target; its cells are pointer_transparent);
  * extra `GroupOrderExternal` at `ggrp` — the group-by proxy (same flatten as
    `hello-grouped-list`, rendered across columns instead of a single line);
  * extra `ScrollBarExternal`.

The witness (§2 #7 scene-as-data): the same grouped flatten renders as a
multi-column grid (each data row paints 3 cells; group headers span them);
collapsing a group hides its cell rows; row selection (by source) survives the
collapse.

  (A) boot — 3 column headers, group headers + Mesh cell rows (3 cells each),
      visible_len 10006.
  (B) select-by-row — click data row source 0; selected == 0.
  (C) collapse a group — visible_len drops by the Mesh count; the group's cell
      rows (and their cells) vanish; the header stays; selection held.
  (D) collapse_all / expand_all — only headers (no cell rows), then restored.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-grouped-grid"
WIN = (400, 540)
N = 10_000
NGROUPS = 6
NCOLS = 3
VISIBLE_BOOT = N + NGROUPS  # 10006
MEMBERS = [1667, 1667, 1667, 1667, 1666, 1666]
GRID_TAG = "ggrid"
GROUP_TAG = "ggrp"
SCROLL_TAG = "ggrid_scroll"
PAUSE = 0.12


def data_sources(snap) -> list[int]:
    out: list[int] = []
    for tag in abs_rects_of(snap):
        # data rows are `ggrid#<source>`; cells are `gcell_<source>_<col>`.
        if tag.startswith(f"{GRID_TAG}#"):
            out.append(int(tag.split("#", 1)[1]))
    return sorted(out)


def group_headers(snap) -> list[int]:
    return sorted(
        int(t.split("#", 1)[1]) for t in abs_rects_of(snap) if t.startswith(f"{GROUP_TAG}#")
    )


def gg(d, path):
    return d.query(f"/{GROUP_TAG}/external/{path}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: columns + grouped cell rows ────────────────────────
        for c in range(NCOLS):
            assert f"{GRID_TAG}_col{c}" in rects, f"column header {c} present"
        assert f"{GROUP_TAG}#0" in rects, "Mesh group header present"
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "6 headers + 10000 data rows")
        assert_eq(gg(tf, "count"), N, "group proxy knows source count")
        assert_eq(gg(tf, "group_count"), NGROUPS, "six groups")
        assert_eq(gg(tf, "kind_at.0"), "header", "pos 0 is a header")
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 (Mesh) leads")
        assert_eq(gg(tf, "member_count_at.0"), MEMBERS[0], "Mesh member count")
        assert_eq(gg(tf, "collapsed_at.0"), False, "group 0 boots expanded")
        assert_eq(gg(tf, "collapsed.0"), False, "per-group flag: group 0 expanded")
        assert_eq(gg(tf, "collapsed.99"), None, "out-of-range group -> Null")
        assert_eq(gg(tf, "source_at.1"), 0, "pos 1 is Mesh's first member (source 0)")
        assert_eq(gg(tf, "kind_at.2"), "data", "pos 2 is also a Mesh data row")
        assert_eq(gg(tf, "source_at.2"), 6, "pos 2 is Mesh's second member (source 6)")
        assert_eq(gg(tf, "group_at.1"), None, "a data row reports no group id")
        assert_eq(gg(tf, "source_at.0"), None, "a header reports no source")
        # The first Mesh data rows and row 0's three cells are painted.
        srcs = data_sources(snap)
        assert srcs[:3] == [0, 6, 12], f"Mesh members lead the grid: {srcs[:3]}"
        for c in range(NCOLS):
            assert f"gcell_0_{c}" in rects, f"row 0 cell {c} present (multi-column rendering)"
        assert len(srcs) < 40, f"virtualized window, got {len(srcs)} data rows"

        # ── (B) select a row (click the row; cells are transparent) ──────
        tf.click(path=f"{GRID_TAG}#0")
        time.sleep(PAUSE)
        assert_eq(tf.query("/external/selected"), 0, "click selected row source 0")

        # ── (C) collapse a group: hide its cell rows, keep selection ─────
        shrunk = VISIBLE_BOOT - MEMBERS[0]  # 8339
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0), shrunk, "collapse Mesh -> 8339")
        assert_eq(gg(tf, "visible_len"), shrunk, "visible_len reflects the collapse")
        assert_eq(gg(tf, "collapsed.0"), True, "group 0 collapsed")
        # The Texture header now follows the Mesh header directly.
        assert_eq(gg(tf, "kind_at.1"), "header", "collapsed: pos 1 is now a header")
        assert_eq(gg(tf, "group_at.1"), 1, "pos 1 is group 1 (Texture) after collapse")
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)
        assert 0 not in data_sources(snap), "collapsed Mesh hides source 0's row"
        assert "gcell_0_0" not in rects, "collapsed Mesh hides source 0's cells"
        assert f"{GROUP_TAG}#0" in rects, "the Mesh header stays"
        assert_eq(tf.query("/external/selected"), 0, "row selection survives the collapse")

        # ── (D) collapse_all / expand_all ───────────────────────────────
        tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0)  # re-expand Mesh
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/collapse_all", None), NGROUPS, "collapse_all -> 6")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(group_headers(snap), [0, 1, 2, 3, 4, 5], "all six group headers visible")
        assert not data_sources(snap), "collapse_all: no data (cell) rows rendered"
        assert "gcell_0_0" not in abs_rects_of(snap), "collapse_all: no cells rendered"
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/expand_all", None), VISIBLE_BOOT, "expand_all -> 10006")
        tf.scroll(SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 0 in data_sources(snap), "source 0's row re-materializes"
        for c in range(NCOLS):
            assert f"gcell_0_{c}" in abs_rects_of(snap), f"row 0 cell {c} re-materializes"
        assert_eq(tf.query("/external/selected"), 0, "still selected after expand")


if __name__ == "__main__":
    run_demo("R845 §5.27 §5.40 — grouped multi-column data grid", body)
