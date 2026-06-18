#!/usr/bin/env python3
"""R1000 §5.23 off-thread external-async-data → repaint seam — `hello-live-data`.

The first framework consumer of the `RepaintSink` seam (R999): a background OS
thread (the sprag PR-3 PTY-reader analog) owns a `Send` shared log buffer; each
`Tick` poke makes it append a line and call `RepaintSink::request_repaint` (via
`use_repaint_sink`). The binding's `view` reads that producer-authoritative
buffer each frame, so the lines are observable as DATA through `scene/snapshot`
(§2 #7 scene-as-data); no pixels.

What this proves (the end-to-end DATA path of the seam): an off-thread
producer's writes cross the thread boundary into the shared buffer and reach
the painted scene the view reads. The autonomous "wake drives an unforced
repaint" half is verified more directly by Rust tests — the
`AppEvent::ExternalRepaint` handler arming a redraw + the example's cross-thread
`request_repaint` test + the pinion-core cross-thread `RepaintSink` tests — than
a snapshot-polling demo could (a `scene/snapshot from=paint` itself re-renders).

ZERO-FLAKE: after a Tick, the producer appends asynchronously; `wait_snap`
polls the paint scene until the new line lands (bounded, generous timeout). The
cross-thread round-trip completes in microseconds, so the poll never races.

Run from the workspace root:
    cargo build -p hello-live-data --release
    python3 tools/demos/r1000_live_data.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

VIEWPORT = (440, 360)

TICK_TAG = "tick"
STATUS_TAG = "log_status"
LIST_TAG = "log_list"

EMDASH = "—"  # mirrors the binding's `\u{2014}`


def row_tag(visible_index: int) -> str:
    return f"log_row_{visible_index}"


def producer_line(seq: int) -> str:
    """Mirror the binding's `producer_line` SSOT."""
    return f"[{seq:03d}] background event #{seq}"


def text_under(snap, tag: str):
    """The first Text content found under the container tagged `tag`."""
    node = find_by_tag(snap, tag)
    if node is None:
        return None

    def first_text(n):
        if not isinstance(n, dict):
            return None
        if n.get("type") == "Text":
            return n.get("content")
        for child in n.get("children", []) or []:
            hit = first_text(child)
            if hit is not None:
                return hit
        return None

    return first_text(node)


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def wait_status(d, expected: str, where: str):
    return wait_snap(
        d,
        lambda s: status_of(s) == expected,
        viewport=VIEWPORT,
        desc=f"status == {expected!r} ({where})",
    )


def body() -> None:
    with RpcSubprocess("hello-live-data") as d:
        # ── boot: empty log, placeholder note, list + button present. ──
        snap = wait_status(
            d, f"No events yet {EMDASH} press Tick", "boot (empty log)"
        )
        assert find_by_tag(snap, LIST_TAG) is not None, "log list present at boot"
        assert find_by_tag(snap, TICK_TAG) is not None, "Tick button present at boot"
        placeholder = text_under(snap, row_tag(0))
        assert placeholder is not None and "Waiting" in placeholder, (
            f"empty-log placeholder shown, got {placeholder!r}"
        )

        # ── Tick: the reducer pokes the producer thread; it appends line 1
        #    off-thread and wakes the shell. The new line reaches the paint
        #    scene the view reads. ───────────────────────────────────────
        d.click(path=TICK_TAG)
        snap = wait_status(d, "1 event", "after first Tick")
        assert_eq(
            text_under(snap, row_tag(0)),
            producer_line(1),
            "first background line is the producer SSOT",
        )

        # ── Tick again: a second off-thread line; the first stays resident
        #    (oldest-first, under MAX_LINES). ───────────────────────────
        d.click(path=TICK_TAG)
        snap = wait_status(d, "2 events", "after second Tick")
        assert_eq(text_under(snap, row_tag(0)), producer_line(1), "row 0 stable")
        assert_eq(
            text_under(snap, row_tag(1)),
            producer_line(2),
            "row 1 is the second background line",
        )
        assert find_by_tag(snap, STATUS_TAG) is not None, "status region persists"


if __name__ == "__main__":
    sys.exit(run_demo("R1000 off-thread live-data → repaint seam", body))
