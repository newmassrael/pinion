#!/usr/bin/env python3
"""R844 §5.27 §5.40 — stacked Model/View proxies (filter -> sort -> group) E2E.

Drives `hello-grouped-sort` via JSON-RPC. Four coordinators over a 10,000-row
virtualized list:

  * primary `VirtualSelectExternal` at `glist` — selection by SOURCE index;
  * extra `ViewSortFilterExternal` at `gsort` — the upstream sort/filter proxy;
  * extra `GroupOrderExternal` at `ggroup` — groups the upstream `order()`
    (R844 GroupOrderState::with_order_source) into collapsible asset-type
    groups;
  * extra `ScrollBarExternal` at `glist_scrollbar`.

The witness (§2 #7 scene-as-data): the proxy stack composes — sorting reorders
the groups + their members (the grouping re-derives over the upstream's new
order, proving the pointer-keyed memo recomputes), filtering shrinks each
group's reported member count, collapsing hides a group's rows; and the
selection (a source index) survives all three stages.

  (A) boot — source order, 6 groups, visible_len 10006, view_len 10000.
  (B) select-by-source — click data row source 0; selected == 0.
  (C) sort descending — group 3 (9999 % 6) leads; source 9999 is the first
      member; visible_len unchanged; selection survives.
  (D) sort back to none — group 0 leads again.
  (E) filter category 0 — view_len 2000, visible_len 2006, every visible data
      row is category 0; the group-0 header reports its FILTERED count (334);
      selection (source 0, category 0) survives.
  (F) collapse group 0 — visible_len drops by 334; source 0's row hidden;
      selection still held.
  (G) expand + clear filter — back to 10006; source 0 re-materializes selected.
  (H) clicked sort header cycles like the RPC path.
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
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-grouped-sort"
WIN = (400, 540)
N = 10_000
NGROUPS = 6
VISIBLE_BOOT = N + NGROUPS  # 10006
LIST_TAG = "glist"
GROUP_TAG = "ggroup"
SORT_TAG = "gsort"
SCROLL_TAG = "glist_scroll"
# group 0 (Mesh) members surviving filter category 0 = #{ i : i % 30 == 0 } = 334.
G0_CAT0 = 334


def present_sources(snap) -> list[int]:
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{LIST_TAG}#"):
            out.append(int(tag.split("#", 1)[1]))
    return sorted(out)


def selected(d):
    return d.query("/external/selected")


def gg(d, path):
    return d.query(f"/{GROUP_TAG}/external/{path}")


def gs(d, path):
    return d.query(f"/{SORT_TAG}/external/{path}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot ─────────────────────────────────────────────────────
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert f"{GROUP_TAG}#0" in rects, "group 0 header present at boot"
        assert f"{SORT_TAG}#cycle" in rects, "sort control present at boot"
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "6 headers + 10000 data rows")
        assert_eq(gg(tf, "count"), N, "group proxy knows source count")
        assert_eq(gg(tf, "group_count"), NGROUPS, "six groups")
        assert_eq(gs(tf, "sort_dir"), "none", "boot unsorted")
        assert_eq(gs(tf, "filter"), None, "boot unfiltered")
        assert_eq(gs(tf, "view_len"), N, "upstream view spans the whole dataset")
        # Boot flattening: pos 0 = Mesh header, pos 1 = its first member source 0.
        assert_eq(gg(tf, "kind_at.0"), "header", "pos 0 is a header")
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 (Mesh) leads source order")
        assert_eq(gg(tf, "source_at.1"), 0, "pos 1 is Mesh's first member (source 0)")
        boot_rows = present_sources(snap)
        assert len(boot_rows) < 30, f"virtualized window, got {len(boot_rows)}"
        assert_eq(boot_rows[:3], [0, 6, 12], "Mesh members in source order")

        # ── (B) select by source index ───────────────────────────────────
        tf.click(path=f"{LIST_TAG}#0")
        wait_until(lambda: selected(tf) == 0, desc="click selected source 0")

        # ── (C) sort descending: regroup over the new order ──────────────
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", None), "ascending", "cycle -> ascending")
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", None), "descending", "cycle -> descending")
        wait_until(
            lambda: gg(tf, "group_at.0") == 3,
            desc="grouping re-derived over the descending order",
        )
        # Descending by name = reverse source order: source 9999 leads; its
        # group is 9999 % 6 == 3. The grouping re-derived over the new upstream
        # order (the pointer-keyed memo recomputed).
        assert_eq(gg(tf, "group_at.0"), 3, "highest source's group (3) leads descending")
        assert_eq(gg(tf, "source_at.1"), 9999, "source 9999 is the first descending member")
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "sorting does not change visible_len")
        assert_eq(selected(tf), 0, "selection survives the sort")

        # ── (D) sort back to none ────────────────────────────────────────
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/cycle_sort", None), "none", "cycle -> unsorted")
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 leads source order again")

        # ── (E) filter category 0: shrink groups, keep selection ─────────
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/set_filter", 0), 2000, "category 0 -> 2000 rows")
        assert_eq(gs(tf, "view_len"), 2000, "upstream view_len reflects the filter")
        wait_until(
            lambda: gg(tf, "visible_len") == NGROUPS + 2000,
            desc="6 headers + 2000 filtered rows",
        )
        assert_eq(gg(tf, "group_at.0"), 0, "group 0 still leads (source 0 survives the filter)")
        assert_eq(gg(tf, "member_count_at.0"), G0_CAT0, "header reports the FILTERED member count")
        snap = tf.snapshot(source="paint", viewport=WIN)
        filt_rows = present_sources(snap)
        assert filt_rows, "filtered view still renders a window"
        assert all(s % 5 == 0 for s in filt_rows), f"every visible row is category 0: {filt_rows}"
        assert_eq(selected(tf), 0, "selection (source 0, category 0) survives the filter")

        # ── (F) collapse group 0: hide its rows, keep selection ──────────
        shrunk = NGROUPS + 2000 - G0_CAT0
        assert_eq(tf.invoke(f"/{GROUP_TAG}/external/toggle_group", 0), shrunk, "collapse Mesh under filter")
        assert_eq(gg(tf, "visible_len"), shrunk, "visible_len drops by the filtered Mesh count")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 0 not in present_sources(snap), "collapsed group 0 hides source 0's row"
        assert_eq(selected(tf), 0, "selection survives collapse (held by source index)")

        # ── (G) expand + clear filter: restore + selection re-materializes ─
        tf.invoke(f"/{GROUP_TAG}/external/expand_all", None)
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/set_filter", None), N, "clear filter -> full view")
        assert_eq(gg(tf, "visible_len"), VISIBLE_BOOT, "view restored")
        tf.scroll(SCROLL_TAG, to=(0, 0))
        wait_snap(
            tf, lambda s: 0 in present_sources(s), viewport=WIN,
            desc="source 0 re-materializes at the top",
        )
        assert_eq(selected(tf), 0, "still selected after the whole stack unwinds")

        # ── (H) clicked sort header cycles like the RPC path ─────────────
        # The sort is unsorted here (none of E/F/G touch it), so a real click on
        # the header must cycle it to exactly ascending (not merely "change it").
        assert_eq(gs(tf, "sort_dir"), "none", "sort is unsorted before the clicked-header step")
        tf.click(path=f"{SORT_TAG}#cycle")
        wait_until(
            lambda: gs(tf, "sort_dir") == "ascending",
            desc="clicked header cycled none -> ascending",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R844 §5.27 §5.40 — stacked proxies: filter -> sort -> group", body))
