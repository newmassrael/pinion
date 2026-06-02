#!/usr/bin/env python3
"""R747 §5.27 §5.40 — sort / filter on a virtualized list E2E.

Drives `hello-virtual-sort` via JSON-RPC. Composes three coordinators over a
10,000-row virtualized list:

  * primary `VirtualSelectExternal` (R746, 2nd consumer) at `vlist` — single
    selection held by SOURCE data index;
  * extra `ViewSortFilterExternal` (R747) at `vsort` — the sort/filter
    view-order proxy mapping visual positions → source indices;
  * extra `ScrollBarExternal` at `vlist_scrollbar`.

The decisive Model/View witness (§2 #7 scene-as-data): selection is held by
source index, so sorting/filtering reorders/​shrinks the *view* while a
selected source row stays selected — its visual position moves, its data
identity does not.

  (A) boot — source order, nothing selected, full view (view_len == N), small
      window only.
  (B) select-by-source — click row source 5; `selected` == 5.
  (C) sort — `invoke cycle_sort` → ascending regroups the window into the
      Alpha block (sources 0,5,10,15,…); the proxy `order` (source_at.*)
      matches the rendered window; selection SURVIVES (still source 5) now at
      a different visual position; descending reverses; a third cycle returns
      to source order.
  (D) selection ⊥ sort ⊥ virtualization — with source 5 selected + ascending,
      scroll far away: source 5 leaves the window/tree, `selected` is STILL 5.
  (E) filter — `invoke set_filter 1` (Bravo) shrinks `view_len` to 2,000 and
      every rendered row is a Bravo source (≡1 mod 5); selection (an Alpha
      source) is held though filtered out; clearing restores the full view.
  (F) clicked sort header — a real `scene/click` on `vsort#cycle` cycles the
      sort exactly like the RPC path (the interactive surface; live native
      XTEST proves the pixel in the separate pass).
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
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-virtual-sort"
WIN = (360, 500)
N = 10_000
PITCH = 32
VP_H = 384  # 12 rows
OVERSCAN = 3
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"
SORT_TAG = "vsort"
PAUSE = 0.12


def present_sources(snap) -> list[int]:
    """The `vlist#<source>` rows in the snapshot (set; order not asserted)."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith("vlist#"):
            out.append(int(tag.split("#", 1)[1]))
    return sorted(out)


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def selected(d):
    return d.query("/external/selected")


def sort_dir(d):
    return d.query("/vsort/external/sort_dir")


def view_len(d):
    return d.query("/vsort/external/view_len")


def source_at(d, pos: int):
    return d.query(f"/vsort/external/source_at.{pos}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: source order, unselected, full view, small window ──
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert f"{SORT_TAG}#cycle" in rects, "sort header present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(selected(tf), None, "nothing selected at boot")
        assert_eq(sort_dir(tf), "none", "boot is unsorted")
        assert_eq(tf.query("/vsort/external/filter"), None, "boot is unfiltered")
        assert_eq(view_len(tf), N, "unfiltered view spans the whole dataset")
        assert_eq(tf.query("/vsort/external/count"), N, "proxy knows source count")
        assert_eq(tf.query("/external/item_count"), N, "selection bound is the source count")

        boot_rows = present_sources(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        assert_eq(boot_rows[:5], [0, 1, 2, 3, 4], "unsorted window is source order")
        assert_eq(source_at(tf, 0), 0, "unsorted: visual 0 == source 0")
        assert_eq(source_at(tf, 1), 1, "unsorted: visual 1 == source 1")

        # ── (B) select by source index ──────────────────────────────────
        tf.click(path=f"{LIST_TAG}#5")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 5, "click selected source 5")

        # ── (C) sort: ascending regroups into the Alpha block ───────────
        assert_eq(tf.invoke("/vsort/external/cycle_sort", None), "ascending", "cycle → ascending")
        assert_eq(sort_dir(tf), "ascending", "sort is ascending")
        time.sleep(PAUSE)
        # The proxy order: Alpha sources (0,5,10,15,…) lead the view.
        assert_eq(source_at(tf, 0), 0, "ascending visual 0 == Alpha source 0")
        assert_eq(source_at(tf, 1), 5, "ascending visual 1 == Alpha source 5")
        assert_eq(source_at(tf, 2), 10, "ascending visual 2 == Alpha source 10")
        assert_eq(source_at(tf, 2000), 1, "ascending: first Bravo (source 1) after 2000 Alphas")
        # The RENDERED window agrees with the proxy order.
        snap = tf.snapshot(source="paint", viewport=WIN)
        asc_rows = present_sources(snap)
        assert all(s % 5 == 0 for s in asc_rows), f"ascending top window is all Alpha: {asc_rows}"
        # Selection SURVIVED the re-sort (same data row, new visual position).
        assert_eq(selected(tf), 5, "selection survives the sort (still source 5)")
        assert 5 in asc_rows, "selected source 5 is in the (Alpha-led) window after sort"
        assert_eq(view_len(tf), N, "sorting does not change the view length")

        # descending reverses; a third cycle returns to source order.
        assert_eq(tf.invoke("/vsort/external/cycle_sort", None), "descending", "cycle → descending")
        assert_eq(source_at(tf, 0), 9999, "descending visual 0 is the highest key (Echo source 9999)")
        assert_eq(selected(tf), 5, "selection survives descending too")
        assert_eq(tf.invoke("/vsort/external/cycle_sort", None), "none", "cycle → unsorted")
        assert_eq(source_at(tf, 0), 0, "back to source order")

        # ── (D) selection ⊥ sort ⊥ virtualization ───────────────────────
        tf.invoke("/vsort/external/cycle_sort", None)  # ascending again
        time.sleep(PAUSE)
        tf.scroll(SCROLL_TAG, to=(0, 4000))
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 5 not in present_sources(snap), "source 5 left the window after scrolling"
        assert_eq(selected(tf), 5, "selection survives though the row is unmaterialized")
        tf.scroll(SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)

        # ── (E) filter: shrink the view to one category ─────────────────
        assert_eq(tf.invoke("/vsort/external/set_filter", 1), 2000, "Bravo filter → 2000 rows")
        assert_eq(view_len(tf), 2000, "view_len reflects the filter")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=WIN)
        filt_rows = present_sources(snap)
        assert filt_rows, "filtered view still renders a window"
        assert all(s % 5 == 1 for s in filt_rows), f"every visible row is a Bravo source: {filt_rows}"
        assert len(filt_rows) < 30, "filtered view is still virtualized"
        # Selection (an Alpha source) is HELD though filtered out of view.
        assert_eq(selected(tf), 5, "selection held while filtered out (selection ⊥ filter)")
        # Clearing restores the full view; the selected row returns.
        assert_eq(tf.invoke("/vsort/external/set_filter", None), N, "clear filter → full view")
        assert_eq(view_len(tf), N, "view restored")

        # ── (F) clicked sort header cycles like the RPC path ────────────
        before = sort_dir(tf)
        tf.click(path=f"{SORT_TAG}#cycle")
        time.sleep(PAUSE)
        after = sort_dir(tf)
        assert before != after, f"clicked sort header changed the sort ({before} → {after})"


if __name__ == "__main__":
    run_demo("R747 §5.27 §5.40 — sort/filter on a virtualized list", body)
