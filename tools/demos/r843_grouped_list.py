#!/usr/bin/env python3
"""R843 §5.27 §5.40 — group-by on a virtualized list E2E.

Drives `hello-grouped-list` via JSON-RPC. Composes three coordinators over a
10,000-row virtualized list partitioned into 6 collapsible asset-type groups:

  * primary `VirtualSelectExternal` (R746 reuse) at `glist` — single selection
    held by SOURCE data index;
  * extra `GroupOrderExternal` (R843) at `ggroup` — the group-by view-order
    proxy: per-group collapse state + the flattened `GroupRow` sequence
    (headers + data rows in one uniform-pitch window);
  * extra `ScrollBarExternal` at `glist_scrollbar`.

The decisive Model/View witness (§2 #7 scene-as-data): the flattening is one
windowable sequence, so a 10k-row grouped list materializes only the visible
window; collapsing a group shrinks `visible_len` by the group's member count
and hides its data rows, but the selection (held by source index) is untouched
— selection ⊥ grouping, both data-indexed.

  (A) boot — all 6 groups expanded, nothing selected, visible_len == 10006,
      small window only; per-position axes (kind/group/label/member_count/
      source) read as data.
  (B) select-by-source — click data row source 6; `selected` == 6.
  (C) collapse a group — `invoke toggle_group 0` shrinks visible_len by the
      Mesh count (1667 → 8339); group 0's data rows leave the window; the
      header stays and now reports collapsed; selection SURVIVES.
  (D) expand — toggle back → 10006; source 6 re-materializes, still selected.
  (E) collapse_all / expand_all — only the 6 headers (visible_len == 6), then
      the full view restored.
  (F) intervene — admin `collapsed.<group>` set/clear is reversible by member
      count.
  (G) clicked header — a real `scene/click` on `ggroup#2` collapses group 2
      exactly like the RPC path (the interactive surface).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-grouped-list"
WIN = (380, 520)
N = 10_000
NGROUPS = 6
# group g member count = #{ i in [0,N) : i % 6 == g }; 10000 = 6*1666 + 4.
MEMBERS = [1667, 1667, 1667, 1667, 1666, 1666]
VISIBLE_BOOT = N + NGROUPS  # 10006
LIST_TAG = "glist"
GROUP_TAG = "ggroup"
SCROLL_TAG = "glist_scroll"


def present_sources(snap) -> list[int]:
    """The `glist#<source>` data rows in the snapshot (sorted set)."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{LIST_TAG}#"):
            out.append(int(tag.split("#", 1)[1]))
    return sorted(out)


def present_headers(snap) -> list[int]:
    """The `ggroup#<group>` header rows in the snapshot (sorted set)."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{GROUP_TAG}#"):
            out.append(int(tag.split("#", 1)[1]))
    return sorted(out)


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def selected(d):
    return d.query("/external/selected")


def g(d, path):
    return d.query(f"/{GROUP_TAG}/external/{path}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: all groups expanded, unselected, full view, window ──
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert f"{GROUP_TAG}#0" in rects, "group 0 header present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(selected(tf), None, "nothing selected at boot")
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT, "6 headers + 10000 data rows")
        assert_eq(g(tf, "count"), N, "proxy knows source count")
        assert_eq(g(tf, "group_count"), NGROUPS, "six groups")
        assert_eq(tf.query("/external/item_count"), N, "selection bound is the source count")

        # Per-position axes read as data (pos 0 = Mesh header, pos 1 = its
        # first data row source 0).
        assert_eq(g(tf, "kind_at.0"), "header", "pos 0 is a header")
        assert_eq(g(tf, "group_at.0"), 0, "pos 0 is group 0")
        assert_eq(g(tf, "label_at.0"), "Mesh", "group 0 label is Mesh")
        assert_eq(g(tf, "member_count_at.0"), MEMBERS[0], "Mesh member count")
        assert_eq(g(tf, "collapsed_at.0"), False, "group 0 boots expanded")
        assert_eq(g(tf, "source_at.0"), None, "a header has no source")
        assert_eq(g(tf, "kind_at.1"), "data", "pos 1 is a data row")
        assert_eq(g(tf, "source_at.1"), 0, "pos 1 is Mesh's first member (source 0)")
        assert_eq(g(tf, "group_at.1"), None, "a data row reports no group id")
        assert_eq(g(tf, "label_at.1"), None, "a data row reports no group label")
        assert_eq(g(tf, "collapsed.0"), False, "per-group flag: group 0 expanded")
        assert_eq(g(tf, "collapsed.99"), None, "out-of-range group → Null")
        # Out-of-range position is present-but-empty; undeclared path absent.
        assert_eq(g(tf, "kind_at.999999"), None, "out-of-range position → Null")

        boot_rows = present_sources(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        # Mesh (group 0) data rows are source indices 0,6,12,… (every 6th).
        assert_eq(boot_rows[:3], [0, 6, 12], "Mesh group's first members lead the window")

        # ── (B) select by source index (click a data row) ───────────────
        tf.click(path=f"{LIST_TAG}#6")
        wait_until(
            lambda: selected(tf) == 6,
            desc="click selected source 6 (a Mesh row)",
        )

        # ── (C) collapse a group: hides its data, keeps selection ───────
        shrunk = VISIBLE_BOOT - MEMBERS[0]  # 8339
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0), shrunk, "collapse Mesh → 8339")
        assert_eq(g(tf, "visible_len"), shrunk, "visible_len reflects the collapse")
        assert_eq(g(tf, "collapsed.0"), True, "group 0 is collapsed")
        assert_eq(g(tf, "collapsed_at.0"), True, "the header reports collapsed")
        # The Texture header now follows the Mesh header directly (pos 1).
        assert_eq(g(tf, "kind_at.1"), "header", "collapsed: pos 1 is now a header")
        assert_eq(g(tf, "group_at.1"), 1, "pos 1 is group 1 (Texture) after collapse")
        snap = wait_snap(
            tf, lambda s: 6 not in present_sources(s), viewport=WIN,
            desc="collapsed Mesh hides source 6's row",
        )
        assert 0 in present_headers(snap) and 1 in present_headers(snap), "headers adjacent now"
        assert_eq(selected(tf), 6, "selection survives the collapse (still source 6)")

        # ── (D) expand: the row re-materializes, still selected ─────────
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0), VISIBLE_BOOT, "expand → 10006")
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT, "view restored")
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf, lambda s: 6 in present_sources(s), viewport=WIN,
            desc="source 6 re-materializes at the top",
        )
        assert_eq(selected(tf), 6, "still selected after expand")

        # ── (E) collapse_all / expand_all ───────────────────────────────
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/collapse_all", None), NGROUPS, "collapse_all → 6")
        assert_eq(g(tf, "visible_len"), NGROUPS, "only the six headers remain")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(present_headers(snap), [0, 1, 2, 3, 4, 5], "all six headers visible")
        assert not present_sources(snap), "collapse_all: no data rows rendered"
        assert_eq(g(tf, "kind_at.3"), "header", "every visible row is a header")
        assert_eq(g(tf, "collapsed.3"), True, "group 3 collapsed by collapse_all")
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/expand_all", None), VISIBLE_BOOT, "expand_all → 10006")
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT, "full view restored")

        # ── (F) intervene: admin collapse is reversible by member count ─
        prefab = NGROUPS - 1  # group 5
        tf.intervene(f"/{GROUP_TAG}/external/collapsed.{prefab}", True)
        assert_eq(g(tf, f"collapsed.{prefab}"), True, "admin collapsed Prefab")
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT - MEMBERS[prefab], "Prefab hidden (1666)")
        tf.intervene(f"/{GROUP_TAG}/external/collapsed.{prefab}", False)
        assert_eq(g(tf, f"collapsed.{prefab}"), False, "admin expanded Prefab")
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT, "view restored")

        # ── (G) clicked header collapses like the RPC path ──────────────
        # The Mesh header sits at visual position 0 (always in-window at
        # offset 0); a real click on it routes the composite arc to the
        # GroupOrderExternal and collapses the group exactly like toggle_group.
        tf.click(path=f"{GROUP_TAG}#0")
        wait_until(
            lambda: g(tf, "collapsed.0") is True,
            desc="clicked Mesh header collapsed it",
        )
        assert_eq(g(tf, "visible_len"), VISIBLE_BOOT - MEMBERS[0], "clicked collapse shrank the view")


if __name__ == "__main__":
    run_demo("R843 §5.27 §5.40 — group-by on a virtualized list", body)
