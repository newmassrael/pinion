#!/usr/bin/env python3
"""R1413 §5.16 §5.28 §5.7 — a media transport over a pinion-chart Timeline.

`hello-transport` puts a play / pause / stop transport under the R1389 timeline
and gives it a LIVE, auto-advancing now-playhead: a §5.28 `TransportClock`
(a `Tickable` registered with the animation driver, the theme-fade / caret-blink
substrate) sweeps the `0..1` playhead linearly while playing. Because the driver
is the §5.28 one, the R724 `scene/tick` RPC frame-steps it DETERMINISTICALLY,
so a wall-clock transport is CI-testable.

The discipline (the r726 indeterminate-sweep lesson): while PLAYING the clock is
never at rest, so the backend keeps painting and real frame ticks confound the
exact fraction — assert MONOTONIC / bounded advance, not a pinned percent.
Exact values are asserted only in an at-rest state where no frame ticks can
sneak in: Stopped (0 %), Paused (frozen), and ended (100 %). That the paused
playhead is provably frozen IS the point — `is_at_rest` gates the frame loop, so
a paused transport receives no ticks at all.

Verification (>= 30 assertions):
  - boot: Stopped, 0 %, playhead at the left;
  - Play: status Playing;
  - scene/tick: the playhead pixel travels rightward (monotonic), stays within
    the plot, and the readout names the clip under it;
  - Pause: status Paused, and a further scene/tick leaves the pixel FROZEN;
  - Play: resumes, the pixel advances past the frozen spot;
  - Stop: Stopped, 0 %, the playhead is back at the left (rewound);
  - Play + a tick past the end: clamps at 100 %, auto-Pauses at the end;
  - Play at the end: rewinds and plays again.

Run from the workspace root:
    cargo build -p hello-transport --release
    python3 tools/demos/r1413_transport.py
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
)

VIEWPORT = (780, 460)

PLAY = "transport_play"
PAUSE = "transport_pause"
STOP = "transport_stop"
STATUS = "transport_status"


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


def _playhead_x(snap: dict) -> int:
    return _node(snap, "timeline.playhead")["rect"]["x"]


def _plot_bounds(snap: dict) -> tuple[int, int]:
    # The top ruler path spans the full plot width — its rect is the x-extent
    # the playhead must stay inside.
    axis = _node(snap, "timeline.axis.x")["rect"]
    return axis["x"], axis["x"] + axis["w"]


def body() -> None:
    with RpcSubprocess("hello-transport", boot_grace=1.5) as tf:
        # ── boot: Stopped, parked at the start ───────────────────────
        snap = _snap(tf)
        assert find_by_tag(snap, "timeline") is not None, "the timeline root"
        for tag in (PLAY, PAUSE, STOP):
            assert find_by_tag(snap, tag) is not None, f"{tag} button present"
        boot_status = _status(snap)
        assert boot_status.startswith("Stopped"), f"boot is Stopped: {boot_status}"
        assert "0%" in boot_status, f"boot playhead at 0%: {boot_status}"
        left, right = _plot_bounds(snap)
        x_boot = _playhead_x(snap)
        # The playhead is a width-2 stroke, so its box x can sit 1px either side
        # of the exact plot edge — a small tolerance guards the extremes.
        assert abs(x_boot - left) <= 2, f"boot playhead at the left ({x_boot} vs {left})"

        # ── Play ─────────────────────────────────────────────────────
        tf.click(path=PLAY)
        snap = _snap(tf)
        assert _status(snap).startswith("Playing"), f"Play -> Playing: {_status(snap)}"

        # ── scene/tick advances the playhead rightward (monotonic) ───
        x_prev = _playhead_x(_snap(tf))
        for step in range(3):
            tf.tick(2.0)
            snap = _snap(tf)
            x_now = _playhead_x(snap)
            assert x_now > x_prev, f"tick {step}: playhead advanced ({x_prev} -> {x_now})"
            assert left - 2 <= x_now <= right + 2, f"playhead stays within the plot ({x_now})"
            assert _status(snap).startswith("Playing"), "still Playing while ticking"
            x_prev = x_now

        # The readout names a clip under the playhead + the header carries a time.
        assert find_by_tag(snap, "timeline.playhead.header") is not None, "time header"
        mid_status = _status(snap)
        assert "t = " in mid_status, f"status carries the playhead time: {mid_status}"
        assert any(
            clip in mid_status for clip in ("intro", "action", "outro", "theme", "ambient")
        ), f"status names the clip under the playhead: {mid_status}"

        # ── Pause freezes the playhead across ticks ──────────────────
        tf.click(path=PAUSE)
        snap = _snap(tf)
        assert _status(snap).startswith("Paused"), f"Pause -> Paused: {_status(snap)}"
        x_paused = _playhead_x(snap)
        pct_paused = _status(snap)
        tf.tick(4.0)  # a paused clock is at rest — this must be a no-op
        snap = _snap(tf)
        assert_eq(_playhead_x(snap), x_paused, "paused playhead is frozen across a tick")
        assert_eq(_status(snap), pct_paused, "paused status unchanged across a tick")

        # ── Play resumes from the frozen spot ────────────────────────
        tf.click(path=PLAY)
        assert _status(_snap(tf)).startswith("Playing"), "resume -> Playing"
        tf.tick(2.0)
        snap = _snap(tf)
        assert _playhead_x(snap) > x_paused, "resume advances past the paused spot"

        # ── Stop rewinds to the start ────────────────────────────────
        tf.click(path=STOP)
        snap = _snap(tf)
        stop_status = _status(snap)
        assert stop_status.startswith("Stopped"), f"Stop -> Stopped: {stop_status}"
        assert "0%" in stop_status, f"Stop rewinds to 0%: {stop_status}"
        assert abs(_playhead_x(snap) - x_boot) <= 1, "Stop returns the playhead to the left"

        # ── Play to the end: clamps at 100 %, auto-Pauses ────────────
        tf.click(path=PLAY)
        assert _status(_snap(tf)).startswith("Playing"), "Play again -> Playing"
        tf.tick(30.0)  # well past the 12 s duration
        snap = _snap(tf)
        end_status = _status(snap)
        assert end_status.startswith("Paused"), f"reaching the end auto-Pauses: {end_status}"
        assert "100%" in end_status, f"the end clamps at 100%: {end_status}"
        x_end = _playhead_x(snap)
        assert x_end > (left + right) // 2, "the ended playhead is at the far right"
        assert left - 2 <= x_end <= right + 2, "the ended playhead stays within the plot"
        # A further tick at the end cannot advance it (at rest, clamped).
        tf.tick(5.0)
        assert_eq(_playhead_x(_snap(tf)), x_end, "the ended playhead is frozen at the end")

        # ── Play at the end replays from the start ───────────────────
        tf.click(path=PLAY)
        snap = _snap(tf)
        assert _status(snap).startswith("Playing"), "Play at the end -> Playing"
        assert _playhead_x(snap) < x_end, "Play at the end rewinds below the end"


if __name__ == "__main__":
    sys.exit(run_demo("R1413 timeline transport", body))
