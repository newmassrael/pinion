#!/usr/bin/env python3
"""R934 §5.22 §5.23 §5.27 unbounded million-row virtualized lazy-load — `hello-million-row`.

Extends R924 (10,000-row lazy-load, *retention* cache) to a *truly unbounded*
million-row source by backing the page cache with an LRU-bounded
`ResourceCache::with_capacity`. Both the rendered node count (virtualization)
and the resident page count (LRU eviction) stay small at any scroll depth.

Proven as DATA through `scene/snapshot` (§2 #7) — no pixels needed:

  - boot: the visible band (rows 0..15) loads through the pump → real rows;
    deep rows are not even rendered (virtualization holds at 1,000,000 rows).
  - the `million_cacheinfo` line reports "Resident pages: k/4" — the LRU bound
    surfaced as queryable data; k never exceeds 4 however deep you scroll.
  - scroll to a fresh deep band → skeletons first, then data (lazy-load).
  - the EVICTION WITNESS: after scrolling far away, scroll back to the top —
    page 0 (long since evicted) re-appears as a skeleton and re-fetches to data.
    In R924's unbounded cache the top band stayed loaded; here eviction makes it
    reload. That observable difference is the proof memory stays bounded.

ZERO-FLAKE: each page fetch is a deterministic deferred future and each
`scene/snapshot from=paint` advances the pump one step, so the demo's own
polling drives skeleton → rows (and re-fetch after eviction) with no wall-clock
race (R923/r761 mechanism). LRU eviction is deterministic given the scripted
scroll sequence.

Run from the workspace root:
    cargo build -p hello-million-row --release
    python3 tools/demos/r934_million_row.py
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
)

EXAMPLE = "hello-million-row"
WIN = (380, 520)
N = 1_000_000
PAGE_SIZE = 100
PITCH = 32
VP_H = 12 * PITCH  # 384
OVERSCAN = 4
CACHE_PAGES = 4
SCROLL_TAG = "million_scroll"
LIST_TAG = "millionrow"
STATUS_TAG = "million_status"
CACHEINFO_TAG = "million_cacheinfo"

KINDS = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
]

# Mirror the binding's escaped non-ASCII glyphs (U+2013 en-dash, U+2026 ellipsis).
DASH = "–"
SKELETON = "Loading…"


def expected_label(i: int) -> str:
    kind, ext = KINDS[i % len(KINDS)]
    size = (i * 37 + 11) % 900 + 8
    return f"asset_{i:07d}.{ext} ({kind}, {size} KB)"


def visible_window(offset: int) -> set[int]:
    """Python mirror of `compute_visible_range` — the windowing SSOT."""
    offset = max(0, offset)
    bottom = offset + VP_H
    max_index = N - 1
    first_visible = min(offset // PITCH, max_index)
    last_visible = min((bottom - 1) // PITCH, max_index)
    first = max(0, first_visible - OVERSCAN)
    last = min(last_visible + OVERSCAN, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
    return {
        int(tag.split("#", 1)[1])
        for tag in abs_rects_of(snap)
        if tag.startswith(f"{LIST_TAG}#")
    }


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def text_under(snap, tag: str):
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def row_text(snap, index: int):
    """The text content of the `millionrow#<index>` row, or None if not rendered."""
    return text_under(snap, f"{LIST_TAG}#{index}")


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def resident_pages(snap) -> int:
    """Parse "Resident pages: k/4" → k — the LRU residency surfaced as data."""
    line = text_under(snap, CACHEINFO_TAG)
    assert line is not None and line.startswith("Resident pages: "), f"cache info line: {line!r}"
    k = line.removeprefix("Resident pages: ").split("/", 1)[0]
    return int(k)


def assert_bounded(snap, where: str) -> None:
    k = resident_pages(snap)
    assert k <= CACHE_PAGES, f"{where}: resident pages {k} exceeds LRU bound {CACHE_PAGES}"


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: the visible band resolves through the pump to real rows ──
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == expected_label(0),
            viewport=WIN,
            desc="boot: row 0 loads to its descriptor",
        )
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        rows = present_rows(snap)
        assert_eq(rows, visible_window(0), "boot band == windowing math (rows 0..15)")
        assert len(rows) < 30, f"virtualized: small window, got {len(rows)} of {N}"
        assert 500_000 not in rows, "a deep row is NOT rendered at boot (virtualization)"
        assert 999_999 not in rows, "the last row is NOT rendered at boot"
        for i in sorted(rows):
            assert_eq(row_text(snap, i), expected_label(i), f"row {i} loaded label")
        assert_eq(status_of(snap), f"Rows 1{DASH}16 of {N}", "status band line (loaded)")
        assert_eq(resident_pages(snap), 1, "boot: exactly one page resident (page 0)")

        # ── scroll to a FRESH deep band → skeletons first, then data ──────
        # row 500000 is page 5000, never fetched yet.
        deep = 500_000 * PITCH
        tf.scroll(SCROLL_TAG, to=(0, deep))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == deep and row_text(s, 500_000) == SKELETON,
            viewport=WIN,
            desc="deep band requested → rows are skeletons",
        )
        rows2 = present_rows(snap)
        assert_eq(rows2, visible_window(deep), "deep band == windowing math")
        assert 0 not in rows2, "top rows scrolled out of the window"
        assert len(rows2) < 30, f"window stays small deep in the list, got {len(rows2)}"
        for i in sorted(rows2):
            assert_eq(row_text(snap, i), SKELETON, f"row {i} skeleton while page loads")
        assert status_of(snap).startswith("Loading rows"), "status shows the loading band"
        assert_bounded(snap, "deep band loading")

        snap = wait_snap(
            tf,
            lambda s: row_text(s, 500_000) == expected_label(500_000),
            viewport=WIN,
            desc="deep page resolves → rows show data",
        )
        assert_eq(scroll_offset(snap), deep, "still at the deep offset after load")
        for i in sorted(present_rows(snap)):
            assert_eq(row_text(snap, i), expected_label(i), f"row {i} resolved label")
        assert_bounded(snap, "deep band loaded")

        # ── scroll through several MORE far bands; memory stays bounded ────
        for row in (200_000, 700_000, 900_000):
            off = row * PITCH
            tf.scroll(SCROLL_TAG, to=(0, off))
            snap = wait_snap(
                tf,
                lambda s, r=row: scroll_offset(s) == r * PITCH
                and row_text(s, r) == expected_label(r),
                viewport=WIN,
                desc=f"far band at row {row} resolves to data",
            )
            assert_eq(row_text(snap, row), expected_label(row), f"row {row} loaded after jump")
            assert_bounded(snap, f"after jump to row {row}")

        # ── EVICTION WITNESS: scroll back to top → page 0 reloads ─────────
        # Page 0 was loaded at boot but has long since been evicted by the far
        # jumps (LRU). In R924's unbounded cache the top band would still be
        # loaded; here it re-appears as a skeleton and re-fetches.
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == 0 and row_text(s, 0) == SKELETON,
            viewport=WIN,
            desc="scroll-back to top: evicted page 0 re-appears as a SKELETON",
        )
        assert_eq(present_rows(snap), visible_window(0), "top window restored")
        assert status_of(snap).startswith("Loading rows"), "top band is re-loading after eviction"
        assert_bounded(snap, "top band re-loading")

        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == expected_label(0),
            viewport=WIN,
            desc="re-fetched page 0 resolves back to data",
        )
        assert_eq(scroll_offset(snap), 0, "still at the top after re-fetch")
        assert_eq(row_text(snap, 0), expected_label(0), "page 0 re-fetched to its descriptor")
        assert_eq(status_of(snap), f"Rows 1{DASH}16 of {N}", "top band loaded again")
        assert_bounded(snap, "top band re-loaded")

        # ── the bottom is reachable; window + cache both stay small ───────
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # clamps to N*pitch - viewport
        max_off = N * PITCH - VP_H
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == max_off and row_text(s, 999_999) == expected_label(999_999),
            viewport=WIN,
            desc="bottom band loads the last (millionth) row",
        )
        rows3 = present_rows(snap)
        assert 999_999 in rows3, "the last row is reachable at the bottom"
        assert_eq(rows3, visible_window(max_off), "bottom band == windowing math")
        assert len(rows3) < 30, f"window small even at the bottom, got {len(rows3)}"
        assert_bounded(snap, "bottom band")


if __name__ == "__main__":
    sys.exit(run_demo("R934 unbounded million-row virtualized lazy-load", body))
