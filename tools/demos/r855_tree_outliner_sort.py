#!/usr/bin/env python3
"""R855 §5.27 §5.12 — scene-outliner sibling sort (composed atop the filter).

Drives hello-tree-filter (the scene-graph outliner) over JSON-RPC. R821 gave it
a recursive search filter; R855 adds a sibling SORT composed *atop* the filter —
the proxy stack sort -> filter -> flatten, all example-level (zero substrate
change): the example reorders each level's children Vec, the existing filter +
virtual-tree flatten render the reordered tree. An editor's scene outliner needs
exactly this (search + name sort over a hierarchy at scale).

  (A) boot — unsorted (source) order; Group00 leads.
  (B) cycle the sort (source -> asc -> desc -> source); the leading row tracks.
  (C) set_sort / intervene jump to a named mode; an unknown mode is rejected.
  (D) sort + filter compose — under a "Node" filter + descending sort, the
      largest group leads and its revealed leaves descend within it.
  (E) clearing the filter keeps the sort; restoring source order.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-tree-filter"
FILTER = "/sgtree_filter/external"
SORT = "/sgtree_sort/external"


def sort_mode(tf):
    return tf.query(f"{SORT}/sort")


def label_at(tf, pos):
    return tf.query(f"{FILTER}/label_at.{pos}")


def visible_count(tf):
    return tf.query(f"{FILTER}/visible_count")


def cycle(tf):
    return tf.invoke(f"{SORT}/cycle_sort", None)


def set_sort(tf, mode):
    return tf.invoke(f"{SORT}/set_sort", mode)


def set_filter(tf, value):
    return tf.invoke(f"{FILTER}/set_filter", value)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: source order ───────────────────────────────────
        assert_eq(sort_mode(tf), "source", "boots in source order")
        assert visible_count(tf) > 0, "boot shows rows"
        assert_eq(label_at(tf, 0), "Group00", "source order: Group00 leads")

        # ── (B) cycle source -> asc -> desc -> source ────────────────
        assert_eq(cycle(tf), "asc", "cycle -> ascending")
        assert_eq(sort_mode(tf), "asc", "sort reads ascending")
        assert_eq(label_at(tf, 0), "Group00", "ascending: Group00 still leads")
        assert_eq(cycle(tf), "desc", "cycle -> descending")
        assert_eq(sort_mode(tf), "desc", "sort reads descending")
        assert_eq(label_at(tf, 0), "Group11", "descending: Group11 leads")
        assert_eq(cycle(tf), "source", "cycle -> source")
        assert_eq(label_at(tf, 0), "Group00", "source order restored")

        # ── (C) set_sort / intervene + guards ────────────────────────
        assert_eq(set_sort(tf, "desc"), "desc", "set_sort jumps to descending")
        assert_eq(label_at(tf, 0), "Group11", "descending leads with Group11")
        assert_eq(set_sort(tf, "asc"), "asc", "set_sort jumps to ascending")
        assert_eq(label_at(tf, 0), "Group00", "ascending leads with Group00")
        rejected = False
        try:
            set_sort(tf, "sideways")
        except RpcError:
            rejected = True
        assert rejected, "an unknown sort mode is rejected"
        tf.intervene(f"{SORT}/sort", "source")
        assert_eq(sort_mode(tf), "source", "intervene restores source order")

        # ── (D) sort + filter compose ────────────────────────────────
        set_filter(tf, "Node")  # every leaf matches → all 12 groups revealed
        assert visible_count(tf) > 12, "filter reveals groups + their leaves"
        set_sort(tf, "desc")
        assert_eq(label_at(tf, 0), "Group11", "filtered + descending: Group11 leads")
        assert_eq(label_at(tf, 1), "Node11_19", "its revealed leaves descend (Node11_19 first)")
        assert_eq(label_at(tf, 2), "Node11_18", "…then Node11_18")
        set_sort(tf, "asc")
        assert_eq(label_at(tf, 0), "Group00", "filtered + ascending: Group00 leads")
        assert_eq(label_at(tf, 1), "Node00_00", "its leaves ascend (Node00_00 first)")

        # ── (E) clearing the filter keeps the sort ───────────────────
        set_sort(tf, "desc")
        set_filter(tf, "")
        assert_eq(sort_mode(tf), "desc", "the sort survives clearing the filter")
        assert_eq(label_at(tf, 0), "Group11", "still descending after the filter clears")
        set_sort(tf, "source")
        assert_eq(label_at(tf, 0), "Group00", "source order restored at the end")


if __name__ == "__main__":
    sys.exit(run_demo("R855 §5.27 §5.12 — scene-outliner sibling sort", body))
