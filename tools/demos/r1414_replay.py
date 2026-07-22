#!/usr/bin/env python3
"""R1414 §5.16 §5.28 §5.7 — a capture-replay tool over the transport substrate.

`hello-replay` is the 2nd consumer of the R1414 `pinion_core::widgets::transport`
`TransportClock` (the play/pause/stop clock lifted out of `hello-transport`). It
drives a DIFFERENT visualization: a `pinion-chart` `LineChart` that progressively
REVEALS a fixed recorded signal up to the playhead over a stable x/y frame, like
scrubbing a video. The reveal count is the observable — it climbs while playing,
freezes while paused, empties on stop, and reaches N at the end.

Same discipline as the transport substrate's own demo (r1413 / r726): while
PLAYING the clock is not at rest, so real frame ticks confound the exact count —
assert MONOTONIC growth while playing, and EXACT counts only in at-rest states
(Stopped -> 0, Paused -> frozen, ended -> N). That the paused reveal is provably
frozen IS the point: `is_at_rest` gates the frame loop.

Verification (>= 30 assertions):
  - boot: Stopped, 0 revealed, the chart + 3 buttons present;
  - Play: status Playing;
  - scene/tick: the revealed count climbs (monotonic) and the polyline lengthens;
  - Pause: the count is FROZEN across a further tick;
  - Play: resumes past the frozen count;
  - Stop: back to 0 revealed;
  - Play + a tick past the end: all N revealed, auto-Paused.

Run from the workspace root:
    cargo build -p hello-replay --release
    python3 tools/demos/r1414_replay.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (760, 460)

PLAY = "replay_play"
PAUSE = "replay_pause"
STOP = "replay_stop"
STATUS = "replay_status"

N_SAMPLES = 60


def _snap(d: RpcSubprocess) -> dict:
    return d.snapshot(source="paint", viewport=VIEWPORT)


def _node(snap: dict, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _status(snap: dict) -> str:
    content = _node(snap, STATUS).get("content")
    assert content is not None, "the status carries a readout string"
    return content


def _revealed(snap: dict) -> int:
    m = re.search(r"revealed (\d+)/(\d+)", _status(snap))
    assert m is not None, f"status names the reveal count: {_status(snap)!r}"
    total = int(m.group(2))
    assert total == N_SAMPLES, f"the recording is {N_SAMPLES} samples, got {total}"
    return int(m.group(1))


def _series_points(snap: dict) -> int:
    """How many vertices the revealed polyline paints — the `chart.series.0`
    path's commands (a single path node, not per-point tags; empty ⇒ 0)."""
    node = find_by_tag(snap, "chart.series.0")
    if node is None:
        return 0
    return sum(1 for c in node.get("commands", []) if "point" in c)


def _max_y_tick(snap: dict) -> float:
    """The largest numeric y-tick label — a proxy for the y-axis frame. A replay
    keeps this FIXED as data reveals (a stable frame), unlike an auto-fit chart."""
    best = 0.0
    for k in range(8):
        node = find_by_tag(snap, f"chart.label.y.{k}")
        if node is None:
            continue
        raw = (node.get("content") or "").strip()
        try:
            best = max(best, abs(float(raw)))
        except ValueError:
            continue
    return best


def body() -> None:
    with RpcSubprocess("hello-replay", boot_grace=1.5) as tf:
        # ── boot: Stopped, nothing revealed ──────────────────────────
        snap = _snap(tf)
        assert find_by_tag(snap, "chart") is not None, "the chart root"
        for tag in (PLAY, PAUSE, STOP):
            assert find_by_tag(snap, tag) is not None, f"{tag} button present"
        boot = _status(snap)
        assert boot.startswith("Stopped"), f"boot is Stopped: {boot}"
        assert_eq(_revealed(snap), 0, "boot reveals nothing")
        # The frame is drawn even with nothing revealed (axes present) — a replay
        # plays back INTO a stable frame, so the axes exist before any data.
        assert find_by_tag(snap, "chart.axis.x") is not None, "x-axis present at boot"
        assert find_by_tag(snap, "chart.axis.y") is not None, "y-axis present at boot"
        boot_ymax = _max_y_tick(snap)
        assert boot_ymax > 0.0, f"the fixed y-frame has ticks at boot: {boot_ymax}"

        # ── Play ─────────────────────────────────────────────────────
        tf.click(path=PLAY)
        assert _status(_snap(tf)).startswith("Playing"), "Play -> Playing"

        # ── scene/tick reveals more samples (monotonic) ──────────────
        prev = _revealed(_snap(tf))
        for step in range(3):
            tf.tick(2.0)
            snap = _snap(tf)
            now = _revealed(snap)
            assert now >= prev, f"tick {step}: reveal did not go backwards ({prev} -> {now})"
            assert now > 0, "the reveal is non-empty while playing past t=0"
            assert _status(snap).startswith("Playing"), "still Playing while ticking"
            # The chart mirrors the status: the polyline has ~one vertex per
            # revealed sample (the reveal is the picture, not just the readout).
            assert abs(_series_points(snap) - now) <= 1, (
                f"the polyline tracks the reveal ({_series_points(snap)} vs {now})"
            )
            prev = now
        assert prev > 0, "the reveal advanced while playing"
        assert _series_points(_snap(tf)) >= 2, "the revealed polyline has vertices"
        # The y-frame is UNCHANGED as data reveals — the stable-frame property
        # that distinguishes a replay from an auto-fit live chart.
        assert_eq(_max_y_tick(_snap(tf)), boot_ymax, "the replay y-frame is fixed")

        # ── Pause freezes the reveal across ticks ────────────────────
        tf.click(path=PAUSE)
        snap = _snap(tf)
        assert _status(snap).startswith("Paused"), f"Pause -> Paused: {_status(snap)}"
        frozen = _revealed(snap)
        tf.tick(3.0)  # a paused clock is at rest — a no-op
        assert_eq(_revealed(_snap(tf)), frozen, "paused reveal is frozen across a tick")

        # ── Play resumes past the frozen count ───────────────────────
        tf.click(path=PLAY)
        assert _status(_snap(tf)).startswith("Playing"), "resume -> Playing"
        tf.tick(2.0)
        assert _revealed(_snap(tf)) > frozen, "resume reveals past the frozen count"

        # ── Stop empties the reveal ──────────────────────────────────
        tf.click(path=STOP)
        snap = _snap(tf)
        assert _status(snap).startswith("Stopped"), f"Stop -> Stopped: {_status(snap)}"
        assert_eq(_revealed(snap), 0, "Stop empties the reveal")

        # ── Play to the end: all N revealed, auto-Paused ─────────────
        tf.click(path=PLAY)
        assert _status(_snap(tf)).startswith("Playing"), "Play again -> Playing"
        tf.tick(30.0)  # well past the 10 s recording
        snap = _snap(tf)
        end = _status(snap)
        assert end.startswith("Paused"), f"reaching the end auto-Pauses: {end}"
        assert_eq(_revealed(snap), N_SAMPLES, "the whole recording is revealed at the end")
        assert "100%" in end, f"the end clamps at 100%: {end}"
        # A further tick at the end cannot reveal more (at rest, clamped).
        tf.tick(5.0)
        assert_eq(_revealed(_snap(tf)), N_SAMPLES, "the ended reveal is frozen at N")


if __name__ == "__main__":
    sys.exit(run_demo("R1414 capture-replay", body))
