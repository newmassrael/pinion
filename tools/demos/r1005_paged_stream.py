#!/usr/bin/env python3
"""R1005 §5.22 §5.23 §5.27 — paged out-of-memory streaming E2E.

Drives `hello-paged-stream` via JSON-RPC. The Model/View-at-scale slice 1b:
a virtualized log whose row count GROWS at runtime AND whose rows are PAGED
behind an LRU-bounded async cache — so memory stays flat while the stream is
unbounded (a GB DLT log, a analyser capture, a `tail -f` too large for RAM).

Proven as DATA through `scene/snapshot` (§2 #7) — no pixels needed:

  * `scene/click pagedstream_emit` appends a 50-line batch (the count grows).
  * tail-follow is stateless (the R1005-lifted at_bottom/follow_tail substrate):
    at the bottom you follow the newest line; scroll up and you pause.
  * the partial TAIL PAGE re-fetches when the stream appends onto it
    (ResourceCache::invalidate, R1005): a row that lands on an already-fetched
    partial page appears after the next emit.
  * the `pagedstream_cacheinfo` line reports "Resident pages: k/4" — the LRU
    bound surfaced as queryable data; k never exceeds 4 however long you stream
    (the OUT-OF-MEMORY witness).

Latency model (ZERO-FLAKE): each page fetch is a deterministic DeferredReady;
`wait_snap` polls snapshots (each advances the pump one step), so a freshly
fetched / re-fetched page resolves skeleton -> rows with no wall-clock race.
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

EXAMPLE = "hello-paged-stream"
WIN = (440, 540)
PAGE_SIZE = 64
CACHE_PAGES = 4
BATCH = 50
ROW_PITCH = 24
LIST_TAG = "pagedstream"
EMIT_TAG = "pagedstream_emit"
STATUS_TAG = "pagedstream_status"
CACHEINFO_TAG = "pagedstream_cacheinfo"
SCROLL_TAG = "pagedstream_scroll"
DOT = "·"
LEVELS = ["INFO ", "WARN ", "ERROR", "DEBUG"]
SKELETON = "Loading…"


def synth_line(i: int) -> str:
    return f"[{i:06d}] {LEVELS[i % 4]} worker: event {i} processed"


def text_under(snap, tag: str):
    """The first Text child's content under the container tagged `tag`."""
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def row_text(snap, index: int):
    return text_under(snap, f"{LIST_TAG}#{index}")


def resident_pages(snap) -> int:
    """Parse "Resident pages: k/4" -> k (the LRU residency as queryable data)."""
    line = text_under(snap, CACHEINFO_TAG)
    assert line is not None and line.startswith("Resident pages: "), f"cache info: {line!r}"
    return int(line.removeprefix("Resident pages: ").split("/", 1)[0])


def assert_bounded(snap, where: str) -> None:
    k = resident_pages(snap)
    assert k <= CACHE_PAGES, f"resident pages {k} exceeds the {CACHE_PAGES} bound {where}"


def emit_to(tf, total: int):
    """Click Emit until the count reaches `total`, gating each on the status."""
    tf.click(path=EMIT_TAG)
    return wait_snap(
        tf,
        lambda s: status_of(s) is not None and status_of(s).startswith(f"{total} lines"),
        viewport=WIN,
        desc=f"emit -> {total} lines",
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: empty stream, paged cache idle ───────────────────
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == f"0 lines {DOT} click Emit to stream",
            viewport=WIN,
            desc="boot: empty stream",
        )
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present"
        assert EMIT_TAG in rects, "emit button present"
        assert_eq(resident_pages(snap), 0, "no pages fetched while empty")

        # ── (B) emit 1: 50 rows on the partial page 0, follow to bottom ─
        snap = emit_to(tf, BATCH)
        snap = wait_snap(
            tf,
            lambda s: row_text(s, BATCH - 1) == synth_line(BATCH - 1),
            viewport=WIN,
            desc="emit 1: newest line (49) resolves",
        )
        assert_eq(status_of(snap), f"{BATCH} lines {DOT} Following (newest)", "following the tail")
        assert row_text(snap, 0) is None, "oldest line is off the top when following"
        assert_bounded(snap, "after emit 1")
        assert_eq(resident_pages(snap), 1, "only the tail page (0) is resident")

        # ── (C) emit 2: the partial tail page re-fetches (invalidate) ──
        # Row 50 does NOT exist at 50 lines; after the 2nd batch page 0 is full
        # and row 50 appears ON page 0 — only because the partial tail page was
        # invalidated + re-fetched (not because a fresh page was added).
        snap = emit_to(tf, 2 * BATCH)
        assert_eq(status_of(snap), f"{2 * BATCH} lines {DOT} Following (newest)", "still following")
        # Scroll so page 0 (rows 48..63) is in the window, let it resolve.
        tf.scroll(SCROLL_TAG, to=(0, 48 * ROW_PITCH))
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 50) == synth_line(50),
            viewport=WIN,
            desc="invalidate witness: row 50 now lives on the re-fetched page 0",
        )
        assert_eq(row_text(snap, 49), synth_line(49), "row 49 (1st batch) still on page 0")
        assert_eq(row_text(snap, 50), synth_line(50), "row 50 (2nd batch) joined page 0")
        assert_bounded(snap, "after the tail-page re-fetch")

        # ── (D) out-of-memory: stream far past the cache bound ─────────
        # The (C) witness scrolled up (pausing follow); return to the bottom so
        # the deep stream follows the tail.
        tf.scroll(SCROLL_TAG, to=(0, 10**9))
        wait_snap(tf, lambda s: "Following" in (status_of(s) or ""),
                  viewport=WIN, desc="resume follow before the deep stream")
        # Emit up to 700 rows (~11 pages) — far more than the 4-page cap.
        total = 2 * BATCH
        while total < 700:
            total += BATCH
            snap = emit_to(tf, total)
        assert_eq(status_of(snap), f"{total} lines {DOT} Following (newest)", "still following deep")
        assert_bounded(snap, "while following a deep stream")
        # Scroll across far bands; residency stays bounded (memory flat).
        for row in (0, 200, 450, 680):
            tf.scroll(SCROLL_TAG, to=(0, row * ROW_PITCH))
            snap = wait_snap(
                tf,
                lambda s, r=row: row_text(s, r) == synth_line(r),
                viewport=WIN,
                desc=f"band at row {row} resolves",
            )
            assert_bounded(snap, f"after scrolling to row {row}")

        # ── (E) eviction witness: page 0 reloaded as a skeleton first ──
        # We just scrolled to a deep band; jump back to the top — page 0 (long
        # since evicted) re-appears as a SKELETON before resolving.
        tf.scroll(SCROLL_TAG, to=(0, 9_999 * ROW_PITCH))  # clamps to the bottom
        wait_snap(tf, lambda s: row_text(s, total - 1) == synth_line(total - 1),
                  viewport=WIN, desc="jump to the newest line")
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == SKELETON,
            viewport=WIN,
            desc="scroll-back to top: evicted page 0 re-appears as a SKELETON",
        )
        assert_bounded(snap, "mid-reload at the top")
        snap = wait_snap(
            tf,
            lambda s: row_text(s, 0) == synth_line(0),
            viewport=WIN,
            desc="evicted page 0 re-fetches to real rows",
        )
        assert_eq(row_text(snap, 0), synth_line(0), "row 0 reloaded")
        assert_eq(status_of(snap), f"{total} lines {DOT} Paused (scroll to bottom to follow)", "paused at top")

        # ── (F) pause/resume tail-follow ───────────────────────────────
        # Paused (at the top): an emit appends below, the viewport stays put.
        snap = emit_to(tf, total + BATCH)
        assert_eq(row_text(snap, 0), synth_line(0), "still at the top: oldest line shown")
        assert "Paused" in status_of(snap), "emit while scrolled up stays paused"
        assert_bounded(snap, "after a paused emit")
        # Resume: scroll to the bottom, the next emit follows again.
        tf.scroll(SCROLL_TAG, to=(0, 10**9))
        wait_snap(tf, lambda s: "Following" in (status_of(s) or ""),
                  viewport=WIN, desc="scroll to bottom resumes following")
        final = total + 2 * BATCH
        snap = emit_to(tf, final)
        assert_eq(status_of(snap), f"{final} lines {DOT} Following (newest)", "follow resumed")
        # Let the newest line's freshly-appended page resolve through the pump.
        snap = wait_snap(
            tf,
            lambda s: row_text(s, final - 1) == synth_line(final - 1),
            viewport=WIN,
            desc="newest line resolves when following",
        )
        assert_eq(row_text(snap, final - 1), synth_line(final - 1), "newest line shown when following")
        assert_bounded(snap, "final state")


if __name__ == "__main__":
    sys.exit(run_demo("R1005 §5.22 §5.23 §5.27 — paged out-of-memory streaming", body))
