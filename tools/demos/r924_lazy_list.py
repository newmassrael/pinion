#!/usr/bin/env python3
"""R924 §5.22 §5.23 §5.27 infinite-scroll virtualized lazy-load — `hello-lazy-list`.

Combines R744 virtualization (10,000 rows, ~16 rendered) with R923 async
paged loading: each 100-row page is fetched asynchronously the first time it
scrolls into view (skeleton "Loading…" until it resolves to data). Proven as
DATA through `scene/snapshot` (§2 #7) — no pixels needed:

  - boot: the visible band (rows 0..15) loads through the pump → real rows;
    deep rows are not even rendered (virtualization holds).
  - scroll to a fresh band: the rows appear as skeletons first, then resolve
    to their descriptors (the async lazy-load).
  - the rendered window stays small even deep in the list.

ZERO-FLAKE: each page fetch is a deterministic deferred future and each
`scene/snapshot from=paint` advances the pump one step, so the demo's own
polling drives skeleton → rows with no wall-clock race (R923/r761 mechanism).

Run from the workspace root:
    cargo build -p hello-lazy-list --release
    python3 tools/demos/r924_lazy_list.py
"""

from __future__ import annotations

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

EXAMPLE = "hello-lazy-list"
WIN = (380, 500)
N = 10_000
PAGE_SIZE = 100
PITCH = 32
VP_H = 12 * PITCH  # 384
OVERSCAN = 4
SCROLL_TAG = "lazy_scroll"
LIST_TAG = "lazylist"
STATUS_TAG = "lazy_status"

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
    return f"asset_{i:05d}.{ext} ({kind}, {size} KB)"


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
        for tag in unclipped_rects_of(snap)
        if tag.startswith(f"{LIST_TAG}#")
    }


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def row_text(snap, index: int):
    """The text content of the `lazylist#<index>` row, or None if not rendered."""
    node = find_by_tag(snap, f"{LIST_TAG}#{index}")
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def status_of(snap):
    node = find_by_tag(snap, STATUS_TAG)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


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
        assert 5000 not in rows, "a deep row is NOT rendered at boot (virtualization)"
        assert 9999 not in rows, "the last row is NOT rendered at boot"
        # Every rendered row in the first band carries its loaded descriptor.
        for i in sorted(rows):
            assert_eq(row_text(snap, i), expected_label(i), f"row {i} loaded label")
        assert_eq(status_of(snap), f"Rows 1{DASH}16 of {N}", "status band line (loaded)")

        # ── scroll to a FRESH deep band → skeletons first, then data ──────
        # row 5000 is page 50, never fetched yet. The Effect (subscribed to the
        # scroll offset) requests it on scroll; the demo's polling drives it
        # from skeleton → data.
        deep = 5000 * PITCH  # offset for row 5000 at the window top
        tf.scroll(SCROLL_TAG, to=(0, deep))
        # First observe the freshly-requested band as skeletons (Loading…).
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == deep and row_text(s, 5000) == SKELETON,
            viewport=WIN,
            desc="deep band requested → rows are skeletons",
        )
        rows2 = present_rows(snap)
        assert_eq(rows2, visible_window(deep), "deep band == windowing math")
        assert 0 not in rows2, "top rows scrolled out of the window"
        assert len(rows2) < 30, f"window stays small deep in the list, got {len(rows2)}"
        # The whole fresh band is skeletons while the page loads.
        for i in sorted(rows2):
            assert_eq(row_text(snap, i), SKELETON, f"row {i} skeleton while page loads")
        assert status_of(snap).startswith("Loading rows"), "status shows the loading band"

        # ── then the page resolves → real descriptors in place ────────────
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 5000) == expected_label(5000),
            viewport=WIN,
            desc="deep page resolves → rows show data",
        )
        assert_eq(scroll_offset(snap), deep, "still at the deep offset after load")
        for i in sorted(present_rows(snap)):
            assert_eq(row_text(snap, i), expected_label(i), f"row {i} resolved label")
        assert_eq(status_of(snap), f"Rows 4997{DASH}5016 of {N}", "status band line (deep, loaded)")

        # ── scroll to the very bottom — last row reachable, window small ──
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # clamps to N*pitch - viewport
        max_off = N * PITCH - VP_H
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == max_off and row_text(s, 9999) == expected_label(9999),
            viewport=WIN,
            desc="bottom band loads the last row",
        )
        rows3 = present_rows(snap)
        assert 9999 in rows3, "the last row is reachable at the bottom"
        assert_eq(rows3, visible_window(max_off), "bottom band == windowing math")
        assert len(rows3) < 30, f"window small even at the bottom, got {len(rows3)}"

        # ── scroll back to top restores the (already-cached) first band ──
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == 0,
            viewport=WIN,
            desc="scrolled back to top",
        )
        # Page 0 was cached on boot, so the top band is real immediately.
        assert_eq(present_rows(snap), visible_window(0), "top window restored")
        assert_eq(row_text(snap, 0), expected_label(0), "cached top row still loaded")


if __name__ == "__main__":
    sys.exit(run_demo("R924 infinite-scroll virtualized lazy-load", body))
