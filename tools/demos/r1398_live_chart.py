#!/usr/bin/env python3
"""R1398 §5.16 §5.23 §5.7 — a LIVE strip chart: off-thread feed + scrolling
window + y-auto-fit, over RPC.

R1000 gave the framework the off-thread `RepaintSink` seam (a background thread
appends data + wakes the shell) and proved it with a log LIST. R1397 gave the
chart `rescale_y_to_x_window` (the y-axis fits the brushed x-window). R1398
composes them into the canonical live telemetry strip chart: `hello-live-chart`'s
producer thread appends `DataPoint`s on each `Tick`, and `view` charts them over
a SLIDING x-window (`window_domain`) with `rescale_y_to_x_window(true)` — R1397's
2nd consumer. A large startup transient (seq 1..3, y ~940) scrolls off the left
as the window follows the newest sample, and the y-axis auto-fits from ~1000 down
to ~tens once the transient is gone.

The proof is scene-as-data (§2 #7), driven by an AI agent (§2 #2), no pixels; the
off-thread appends are awaited with a bounded poll (ZERO-FLAKE, the R1000
pattern — the cross-thread round-trip completes in microseconds):

  (A) boot — the chart root is present, the status is the empty placeholder.
  (B) Tick into the transient (8 samples) — the window is anchored at x=1, the
      y-axis reaches the transient magnitude (>=500), the polyline is drawn.
  (C) Tick until the window scrolls past the transient (30 samples) — the window
      left edge has advanced to x=18, the y-axis auto-fits DOWN to tens (a >3x
      drop), and the visible polyline is BOUNDED to the window (far fewer than
      30 vertices) — the strip clips, it does not grow without bound.

Run from the workspace root:
    cargo build -p hello-live-chart --release
    python3 tools/demos/r1398_live_chart.py

>= 30 assertions.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_snap,
)

VIEWPORT = (760, 460)
TICK_TAG = "tick"
STATUS_TAG = "live_status"


def paint(d):
    return d.snapshot(source="paint", viewport=VIEWPORT)


def status_of(snap) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "the status live region is present"
    return node.get("content") or ""


def count_of(snap) -> int:
    m = re.search(r"(\d+) samples", status_of(snap))
    return int(m.group(1)) if m else 0


def window_of(snap) -> tuple[int, int]:
    m = re.search(r"window x (\d+)\.\.(\d+)", status_of(snap))
    assert m is not None, f"the status states the window, got {status_of(snap)!r}"
    return int(m.group(1)), int(m.group(2))


def _tick_value(text: str):
    raw = (text or "").strip()
    mul = 1.0
    for suffix, factor in (("k", 1_000.0), ("M", 1_000_000.0)):
        if raw.endswith(suffix):
            raw, mul = raw[: -len(suffix)], factor
            break
    try:
        return abs(float(raw.strip())) * mul
    except ValueError:
        return None


def max_y_tick(snap) -> float:
    best = 0.0
    for k in range(12):
        node = find_by_tag(snap, f"chart.label.y.{k}")
        if node is None:
            continue
        v = _tick_value(node.get("content"))
        if v is not None:
            best = max(best, v)
    return best


def series_vertex_count(snap) -> int:
    node = find_by_tag(snap, "chart.series.0")
    if node is None:
        return 0
    return sum(1 for c in node.get("commands", []) if c["type"] in ("MoveTo", "LineTo"))


def advance_to(d, target: int):
    """Click Tick until the resident sample count reaches `target`, awaiting each
    off-thread append (bounded poll, ZERO-FLAKE)."""
    snap = paint(d)
    while count_of(snap) < target:
        cur = count_of(snap)
        d.click(path=TICK_TAG)
        snap = wait_snap(
            d,
            lambda s, c=cur: count_of(s) > c,
            viewport=VIEWPORT,
            desc=f"sample count > {cur}",
        )
    return snap


def body() -> None:
    with RpcSubprocess("hello-live-chart") as d:
        # ── (A) boot — empty ring, chart present, placeholder status ─────────
        snap = paint(d)
        assert find_by_tag(snap, "chart") is not None, "the chart root is present at boot"
        assert find_by_tag(snap, TICK_TAG) is not None, "the Tick button is present"
        assert "No samples yet" in status_of(snap), f"boot placeholder, got {status_of(snap)!r}"
        assert count_of(snap) == 0, "no samples at boot"
        assert find_by_tag(snap, "chart.axis.y") is not None, "the chart has axes even when empty"
        assert series_vertex_count(snap) == 0, "no polyline until data arrives"

        # ── (B) Tick into the transient — 8 samples, window anchored at x=1 ───
        snap = advance_to(d, 8)
        assert count_of(snap) == 8, "eight samples streamed in"
        lo, hi = window_of(snap)
        assert lo == 1, f"the window is anchored at the first sample, got {lo}"
        assert hi == 13, f"the window is [1, 1+span], got {hi}"
        assert hi - lo == 12, f"the window width is the span (12), got {hi - lo}"
        early_top = max_y_tick(snap)
        assert early_top >= 500.0, f"the early window's y-axis reaches the transient, got {early_top}"
        assert any(
            (find_by_tag(snap, f"chart.label.y.{k}") or {}).get("content", "").endswith("k")
            for k in range(12)
        ), "the transient window carries a 'k' (kilo) tick label"
        assert find_by_tag(snap, "chart.series.0") is not None, "the polyline is drawn"
        assert series_vertex_count(snap) >= 6, "the early samples are plotted"

        # ── (C) Tick until the window scrolls past the transient — 30 samples ─
        snap = advance_to(d, 30)
        assert count_of(snap) == 30, "thirty samples streamed in"
        lo, hi = window_of(snap)
        assert hi == 30, f"the window right edge follows the newest sample, got {hi}"
        assert lo == 18, f"the window scrolled to [last-span, last], got left={lo}"
        assert lo > 1, "the window's left edge ADVANCED (the strip scrolled)"
        assert hi - lo == 12, f"the window width is preserved while scrolling, got {hi - lo}"
        late_top = max_y_tick(snap)
        assert late_top < 200.0, f"the scrolled window auto-fits the y-axis to tens, got {late_top}"
        assert not any(
            (find_by_tag(snap, f"chart.label.y.{k}") or {}).get("content", "").endswith("k")
            for k in range(12)
        ), "no 'k' tick label survives once the transient scrolls off"
        assert early_top > late_top * 3.0, (
            f"the y-axis auto-fit DOWN as the transient scrolled off "
            f"({early_top} -> {late_top})"
        )
        # The visible polyline is bounded to the window — the strip clips, it
        # does not accumulate all 30 samples.
        visible = series_vertex_count(snap)
        assert visible < 20, f"the visible polyline is bounded to the window, got {visible} vertices"
        assert visible >= 10, f"the window is still fully backed by samples, got {visible}"

        # ── the status live region keeps announcing the latest value ─────────
        assert "latest" in status_of(snap), "the status announces the latest value (a11y live region)"

        # ── (D) a few more Ticks — the window keeps scrolling, y stays fitted ─
        snap = advance_to(d, 36)
        lo2, hi2 = window_of(snap)
        assert hi2 == 36 and lo2 == 24, f"the window kept scrolling to [24, 36], got [{lo2}, {hi2}]"
        assert hi2 - lo2 == 12, "the window width stays the span as it scrolls"
        assert max_y_tick(snap) < 200.0, "the y-axis stays fitted to the steady state"
        assert count_of(snap) == 36, "the sample count kept growing"
        assert count_of(snap) > 30, "the total received kept growing past the window size"


if __name__ == "__main__":
    sys.exit(run_demo("R1398 live strip chart: off-thread feed + scroll + y-auto-fit", body))
