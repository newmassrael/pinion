#!/usr/bin/env python3
"""R996 §5.22 §5.23 §5.27 Model-View streaming-append — `hello-streaming-log`.

A virtualized log whose row count GROWS at runtime. Proven as DATA through
`scene/snapshot` (§2 #7) — no pixels needed:

  - boot: 0 lines, status prompts the first Emit; no rows rendered.
  - click Emit: a batch of 50 lines appends; because the viewport was at the
    (empty) bottom, tail-follow pins it to the NEW bottom — the newest line is
    visible, the oldest scrolled off, the window stays small (virtualized), and
    the scroll extent grew.
  - scroll UP to read history → "Paused": the next Emit appends silently below;
    the viewport stays put and the oldest line stays visible.
  - scroll back to the bottom → "Following" resumes; the next Emit pins to the
    new bottom again.

Tail-follow is STATELESS — derived from the scroll position (`at_bottom`), not a
stored flag. ZERO-FLAKE: every step is a deterministic `scene/click` (append) or
`scene/scroll` (pause/resume); no wall-clock, no async pump.

Run from the workspace root:
    cargo build -p hello-streaming-log --release
    python3 tools/demos/r996_streaming_log.py
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

EXAMPLE = "hello-streaming-log"
WIN = (440, 540)
PITCH = 24
VP_H = 14 * PITCH  # 336
OVERSCAN = 4
BATCH = 50
LOG_TAG = "streamlog"
STATUS_TAG = "stream_status"
EMIT_TAG = "stream_emit"
SCROLL_TAG = "stream_scroll"

# Mirror the binding's escaped middle-dot (U+00B7) status separator.
DOT = "·"
# Mirror the binding's per-line level rotation (5-char padded).
LEVELS = ["INFO ", "WARN ", "ERROR", "DEBUG"]


def expected_line(i: int) -> str:
    return f"[{i:06d}] {LEVELS[i % len(LEVELS)]} worker: event {i} processed"


def content_height(count: int) -> int:
    return count * PITCH


def max_offset(count: int) -> int:
    return max(0, content_height(count) - VP_H)


def visible_window(offset: int, count: int) -> set[int]:
    """Python mirror of `compute_visible_range` against the current count."""
    if count == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + VP_H
    max_index = count - 1
    first_visible = min(offset // PITCH, max_index)
    last_visible = min((bottom - 1) // PITCH, max_index)
    first = max(0, first_visible - OVERSCAN)
    last = min(last_visible + OVERSCAN, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
    return {
        int(tag.split("#", 1)[1])
        for tag in abs_rects_of(snap)
        if tag.startswith(f"{LOG_TAG}#")
    }


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def _text_of(snap, tag: str):
    node = find_by_tag(snap, tag)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def row_text(snap, index: int):
    return _text_of(snap, f"{LOG_TAG}#{index}")


def status_of(snap):
    return _text_of(snap, STATUS_TAG)


def following_status(count: int) -> str:
    return f"{count} lines {DOT} Following (newest)"


def paused_status(count: int) -> str:
    return f"{count} lines {DOT} Paused (scroll to bottom to follow)"


def assert_band_labels(snap, rows: set[int]) -> None:
    """Every rendered row carries its synthesized log line (the data witness)."""
    for i in sorted(rows):
        assert_eq(row_text(snap, i), expected_line(i), f"row {i} line text")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: empty log, prompt for the first Emit ──────────────────────
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == f"0 lines {DOT} click Emit to stream",
            viewport=WIN,
            desc="boot: empty log prompts Emit",
        )
        assert_eq(present_rows(snap), set(), "no rows rendered before any Emit")
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")

        # ── click Emit → 50 lines, tail-follow pins to the new bottom ───────
        tf.click(path=EMIT_TAG)
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == following_status(BATCH),
            viewport=WIN,
            desc="emit 1: 50 lines, following the newest",
        )
        off1 = max_offset(BATCH)
        assert_eq(scroll_offset(snap), off1, "followed to the new bottom (50 rows)")
        assert off1 > 0, "50 rows exceed the 14-row viewport"
        rows = present_rows(snap)
        assert_eq(rows, visible_window(off1, BATCH), "window == windowing math at the tail")
        assert len(rows) < 30, f"virtualized: small window, got {len(rows)}"
        assert (BATCH - 1) in rows, "the newest line (49) is visible when following"
        assert 0 not in rows, "the oldest line (0) scrolled off the top"
        assert_eq(row_text(snap, BATCH - 1), expected_line(BATCH - 1), "newest line text")
        assert_band_labels(snap, rows)

        # ── click Emit again → 100 lines, still following ───────────────────
        tf.click(path=EMIT_TAG)
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == following_status(2 * BATCH),
            viewport=WIN,
            desc="emit 2: 100 lines, still following",
        )
        off2 = max_offset(2 * BATCH)
        assert off2 > off1, "the scroll extent grew with the log"
        assert_eq(scroll_offset(snap), off2, "followed to the new (deeper) bottom")
        rows2 = present_rows(snap)
        assert_eq(rows2, visible_window(off2, 2 * BATCH), "window == windowing math (100 rows)")
        assert (2 * BATCH - 1) in rows2, "the newest line (99) is visible"
        assert_band_labels(snap, rows2)

        # ── scroll UP to the top → follow PAUSES ────────────────────────────
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == paused_status(2 * BATCH),
            viewport=WIN,
            desc="scroll to top → paused",
        )
        assert_eq(scroll_offset(snap), 0, "at the top")
        top_rows = present_rows(snap)
        assert_eq(top_rows, visible_window(0, 2 * BATCH), "top window == windowing math")
        assert 0 in top_rows, "the oldest line is visible at the top"
        assert (2 * BATCH - 1) not in top_rows, "the newest line is off the bottom now"
        assert_eq(row_text(snap, 0), expected_line(0), "oldest line text")

        # ── click Emit while PAUSED → appends below, viewport stays put ─────
        tf.click(path=EMIT_TAG)
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == paused_status(3 * BATCH),
            viewport=WIN,
            desc="emit while paused: 150 lines, viewport stays",
        )
        assert_eq(scroll_offset(snap), 0, "paused: viewport did NOT jump to the tail")
        assert_eq(
            present_rows(snap),
            visible_window(0, 3 * BATCH),
            "paused window unchanged at the top",
        )
        assert 0 in present_rows(snap), "the oldest line is still visible (history preserved)"
        assert_eq(row_text(snap, 0), expected_line(0), "oldest line still shows its text")

        # ── scroll back to the bottom → follow RESUMES ──────────────────────
        tf.scroll(SCROLL_TAG, to=(0, 10**9))  # clamps to the bottom
        off3 = max_offset(3 * BATCH)
        snap = wait_snap(
            tf,
            lambda s: scroll_offset(s) == off3 and status_of(s) == following_status(3 * BATCH),
            viewport=WIN,
            desc="scroll to bottom → following resumes",
        )
        assert (3 * BATCH - 1) in present_rows(snap), "the newest line (149) is back in view"

        # ── click Emit → follows to the new bottom again ────────────────────
        tf.click(path=EMIT_TAG)
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == following_status(4 * BATCH),
            viewport=WIN,
            desc="emit 4: 200 lines, followed to the new bottom",
        )
        off4 = max_offset(4 * BATCH)
        assert_eq(scroll_offset(snap), off4, "followed to the newest bottom")
        rows4 = present_rows(snap)
        assert_eq(rows4, visible_window(off4, 4 * BATCH), "window == windowing math (200 rows)")
        assert (4 * BATCH - 1) in rows4, "the newest line (199) is visible"
        assert len(rows4) < 30, f"window stays small at 200 rows, got {len(rows4)}"
        assert_band_labels(snap, rows4)


if __name__ == "__main__":
    sys.exit(run_demo("R996 Model-View streaming-append", body))
