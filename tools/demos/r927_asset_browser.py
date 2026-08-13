#!/usr/bin/env python3
"""R927 §5.22 §5.23 §5.27 async source-side sort/filter — `hello-asset-browser`.

A virtualized 10,000-row asset browser whose **Sort and Filter are query
parameters pushed down to an out-of-memory source** (not client-side proxies
over a materialized Vec). Proven as DATA through `scene/snapshot` (§2 #7) — no
pixels needed:

  - boot: the filtered count resolves ("10000 assets"), then page 0 loads its
    rows through the pump; deep rows are not even rendered (virtualization).
  - Filter → Texture: the source is re-queried — the count changes to the
    *filtered* count (1667), the scroll resets to the top, and every visible
    row is a Texture (its source index is k + pos*6, the closed-form mapping).
  - Sort → Size asc: the page is re-queried in a *global* size order — the
    re-queried page appears as skeletons first (async), then resolves to rows
    whose sizes increase monotonically; the count is unchanged (sorting does
    not change membership).

ZERO-FLAKE: each count + page fetch is a deterministic deferred future and each
`scene/snapshot from=paint` advances the pump one step, so the demo's own
polling drives Counting → rows / skeleton → data with no wall-clock race
(R923/R924 mechanism).

Run from the workspace root:
    cargo build -p hello-asset-browser --release
    python3 tools/demos/r927_asset_browser.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    unclipped_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-asset-browser"
WIN = (420, 560)
N_TOTAL = 10_000
PAGE_SIZE = 100
PITCH = 32
VP_H = 12 * PITCH  # 384
OVERSCAN = 4
SCROLL_TAG = "ab_scroll"
LIST_TAG = "assetbrowser"
COUNT_TAG = "ab_count"
SORT_TAG = "ab_sort"
FILTER_TAG = "ab_filter"

KINDS = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
]

SKELETON = "Loading…"  # mirror the binding's escaped U+2026 ellipsis


def expected_label(i: int) -> str:
    """Python mirror of `row_at` + `row_label` — the painted-text SSOT."""
    kind, ext = KINDS[i % len(KINDS)]
    size = (i * 37 + 11) % 900 + 8
    return f"asset_{i:05d}.{ext} ({kind}, {size} KB)"


def matching_count(filter_k: int | None) -> int:
    """Python mirror of `matching_count` — the filtered count query."""
    if filter_k is None:
        return N_TOTAL
    base = N_TOTAL // len(KINDS)
    rem = N_TOTAL % len(KINDS)
    return base + (1 if filter_k < rem else 0)


def natural_index(filter_k: int | None, pos: int) -> int:
    """Python mirror of `natural_index` — source index at a visual position."""
    if filter_k is None:
        return pos
    return filter_k + pos * len(KINDS)


def visible_window(offset: int, n: int) -> set[int]:
    """Python mirror of `compute_visible_range` over an item count of `n`."""
    if n == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + VP_H
    max_index = n - 1
    first_visible = min(offset // PITCH, max_index)
    last_visible = min((bottom - 1) // PITCH, max_index)
    first = max(0, first_visible - OVERSCAN)
    last = min(last_visible + OVERSCAN, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
    return {
        int(tag.split("#", 1)[1])
        for tag in unclipped_rects_of(snap)
        if tag.startswith(f"{LIST_TAG}#")
    }


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def _text_under(snap, tag: str):
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def row_text(snap, pos: int):
    return _text_under(snap, f"{LIST_TAG}#{pos}")


def count_text(snap):
    return _text_under(snap, COUNT_TAG)


def size_of_label(label: str) -> int:
    """Pull the KB size out of a row label `… (Kind, NNN KB)`."""
    m = re.search(r"\((?:[^,]+), (\d+) KB\)$", label)
    assert m is not None, f"unparsable row label: {label!r}"
    return int(m.group(1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: the count resolves, then page 0 loads through the pump ──
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == expected_label(0),
            viewport=WIN,
            desc="boot: row 0 loads to its descriptor",
        )
        assert_eq(count_text(snap), "10000 assets", "boot count line (unfiltered)")
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        rows = present_rows(snap)
        assert_eq(rows, visible_window(0, N_TOTAL), "boot band == windowing over 10000")
        assert len(rows) < 30, f"virtualized: small window, got {len(rows)} of {N_TOTAL}"
        assert 5000 not in rows, "a deep row is NOT rendered at boot (virtualization)"
        assert 9999 not in rows, "the last row is NOT rendered at boot"
        for i in sorted(rows):
            assert_eq(row_text(snap, i), expected_label(i), f"row {i} natural label")

        # ── Filter → Texture (kind 0): re-query the SOURCE ──
        # The count becomes the filtered count, the scroll resets, and every
        # row is a Texture whose source index is the closed-form k + pos*6.
        tf.click(path=FILTER_TAG)
        snap = wait_snap(
            tf,
            lambda s: count_text(s) == "1667 Texture assets" and row_text(s, 0) == expected_label(0),
            viewport=WIN,
            desc="filter Texture: re-queried count + rows",
        )
        assert_eq(scroll_offset(snap), 0, "a re-query jumps back to the top")
        tex_rows = present_rows(snap)
        assert_eq(
            tex_rows,
            visible_window(0, matching_count(0)),
            "filtered band == windowing over the filtered count",
        )
        for j in sorted(tex_rows):
            src = natural_index(0, j)
            assert_eq(row_text(snap, j), expected_label(src), f"pos {j} == source row {src}")
            assert "Texture" in row_text(snap, j), f"row {j} is a Texture"

        # ── Filter → Mesh (kind 1): membership + count change again ──
        tf.click(path=FILTER_TAG)
        snap = wait_snap(
            tf,
            lambda s: count_text(s) == "1667 Mesh assets" and row_text(s, 0) == expected_label(1),
            viewport=WIN,
            desc="filter Mesh: re-queried to the Mesh set",
        )
        assert_eq(row_text(snap, 0), expected_label(1), "Mesh pos 0 == source row 1 (asset_00001)")
        for j in sorted(present_rows(snap)):
            assert "Mesh" in row_text(snap, j), f"row {j} is a Mesh"

        # ── Sort → Size asc (Filter stays Mesh): the page is re-queried ──
        # First the re-queried (SizeAsc, Mesh, 0) page is a skeleton (async),
        # then it resolves to a GLOBAL ascending size order — the count is
        # unchanged because sorting does not change membership.
        tf.click(path=SORT_TAG)
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == SKELETON,
            viewport=WIN,
            desc="sort Size asc: re-queried page is a skeleton first",
        )
        assert_eq(count_text(snap), "1667 Mesh assets", "sort keeps the filtered count")
        assert_eq(scroll_offset(snap), 0, "sort reset the scroll")
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) is not None and row_text(s, 0) != SKELETON,
            viewport=WIN,
            desc="sort Size asc: page resolves to sorted rows",
        )
        assert_eq(count_text(snap), "1667 Mesh assets", "count still the Mesh count after sort")
        sized = [
            size_of_label(row_text(snap, j))
            for j in sorted(present_rows(snap))
            if row_text(snap, j) not in (None, SKELETON)
        ]
        assert len(sized) >= 12, f"a full window of sorted rows, got {len(sized)}"
        assert sized == sorted(sized), f"global ascending size order: {sized}"
        for j in sorted(present_rows(snap)):
            assert "Mesh" in row_text(snap, j), f"sorted row {j} is still a Mesh"

        # ── deep scroll within the sorted+filtered view stays virtualized ──
        deep = 800 * PITCH  # well past the first page, within the Mesh count
        tf.scroll(SCROLL_TAG, to=(0, deep))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == deep
            and all(rt not in (None, SKELETON) for rt in [row_text(s, p) for p in present_rows(s)]),
            viewport=WIN,
            desc="deep band within the filtered view loads",
        )
        deep_rows = present_rows(snap)
        assert_eq(deep_rows, visible_window(deep, matching_count(1)), "deep band == windowing math")
        assert len(deep_rows) < 30, f"window stays small deep in the list, got {len(deep_rows)}"
        assert 0 not in deep_rows, "top rows scrolled out of the window"
        for j in sorted(deep_rows):
            assert "Mesh" in row_text(snap, j), f"deep row {j} is still a Mesh"
        deep_sizes = [size_of_label(row_text(snap, j)) for j in sorted(deep_rows)]
        assert deep_sizes == sorted(deep_sizes), f"deep band still ascending: {deep_sizes}"

        # ── back to the top: the (cached) first sorted page is immediate ──
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == 0 and row_text(s, 0) not in (None, SKELETON),
            viewport=WIN,
            desc="scrolled back to the top (cached first page)",
        )
        assert_eq(
            present_rows(snap),
            visible_window(0, matching_count(1)),
            "top window restored over the filtered count",
        )
        assert "Mesh" in row_text(snap, 0), "cached top row still a Mesh"


if __name__ == "__main__":
    sys.exit(run_demo("R927 async source-side sort/filter asset browser", body))
