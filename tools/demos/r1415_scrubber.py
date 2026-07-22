#!/usr/bin/env python3
"""R1415 §5.28 §5.38 §5.16 — a scrub / seek bar over the transport substrate.

`hello-scrubber` is the forcing consumer of the new `TransportClock::seek`, the
scrub verb R1414's substrate lacked. A `SliderExternal` (the R1389 timeline-
scrubber idiom) is reused as a transparent 1-D capture over a seek bar; every
`value_changing` intent — from a drag, `scene/intervene`, or an Arrow key — maps
onto `clock.seek`, so the bar's fill + knob (painted from `clock.position()`)
jump to wherever the drag lands. A play/pause toggle exercises the one seek
semantic a static bar cannot: seeking while playing keeps playing from the new
spot (jump-and-continue).

Same discipline as the transport substrate's own demos (r1413 / r1414 / r726):
while PLAYING the clock is not at rest, so real frame ticks confound an exact
percent — assert MONOTONIC growth while playing and a big unambiguous JUMP on a
seek, and EXACT values only in at-rest states (Stopped / Paused).

Verification (>= 30 assertions):
  - boot: Stopped, 0 %, an empty fill, the bar + toggle present;
  - seek (scene/intervene the scrub value): a stopped clock Pauses at the sought
    spot, the fill grows / shrinks with the fraction, the knob tracks it;
  - Play (toggle): Playing, the toggle relabels to Pause, scene/tick advances;
  - seek while playing: a big jump, yet still Playing (jump-and-continue);
  - Pause (toggle): Paused, frozen across a tick, the toggle relabels to Play;
  - seek to the end then Play: rewinds and replays (the one end-of-clip rule).

Run from the workspace root:
    cargo build -p hello-scrubber --release
    python3 tools/demos/r1415_scrubber.py
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
    rect_of,
    run_demo,
)

VIEWPORT = (760, 280)

SCRUB = "scrubber_scrub"
TOGGLE = "scrubber_toggle"
STATUS = "scrubber_status"
TRACK = "scrubber.track"
FILL = "scrubber.fill"
KNOB = "scrubber.playhead"


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


def _pct(snap: dict) -> int:
    m = re.search(r"(\d+)%", _status(snap))
    assert m is not None, f"the status names a percent: {_status(snap)!r}"
    return int(m.group(1))


def _state_word(snap: dict) -> str:
    return _status(snap).split()[0]


def _fill_w(snap: dict) -> int:
    node = find_by_tag(snap, FILL)
    if node is None:
        return 0
    return int(rect_of(node)["w"])


def _knob_x(snap: dict) -> int:
    return int(rect_of(_node(snap, KNOB))["x"])


def _texts(node: object) -> list[str]:
    out: list[str] = []
    if isinstance(node, dict):
        content = node.get("content")
        if isinstance(content, str):
            out.append(content)
        for child in node.get("children", []) or []:
            out.extend(_texts(child))
    return out


def _toggle_label(snap: dict) -> str | None:
    for text in _texts(_node(snap, TOGGLE)):
        if text in ("Play", "Pause"):
            return text
    return None


def _seek(d: RpcSubprocess, fraction: float) -> None:
    """Drive the primary scrub Slider's value over RPC — the same
    `value_changing` path a pointer drag fires, which the reducer seeks."""
    d.intervene("/external/value", fraction)


def body() -> None:
    with RpcSubprocess("hello-scrubber", boot_grace=1.5) as tf:
        # ── boot: Stopped, nothing sought ────────────────────────────
        snap = _snap(tf)
        assert find_by_tag(snap, TRACK) is not None, "the bar track"
        assert find_by_tag(snap, SCRUB) is not None, "the scrub capture surface"
        assert find_by_tag(snap, TOGGLE) is not None, "the play/pause toggle"
        assert _state_word(snap) == "Stopped", f"boot is Stopped: {_status(snap)}"
        assert_eq(_pct(snap), 0, "boot playhead at 0%")
        assert_eq(_fill_w(snap), 0, "boot fill is empty")
        assert _toggle_label(snap) == "Play", "a stopped toggle offers Play"

        # ── seek a stopped clock: it Pauses at the sought spot ───────
        _seek(tf, 0.4)
        snap = _snap(tf)
        assert _state_word(snap) == "Paused", f"a seek pauses a stopped clock: {_status(snap)}"
        assert_eq(_pct(snap), 40, "the readout names the sought 40%")
        fill_40 = _fill_w(snap)
        knob_40 = _knob_x(snap)
        assert fill_40 > 0, f"a 40% seek fills the bar: {fill_40}"

        # ── seek further right: fill + knob advance ──────────────────
        _seek(tf, 0.8)
        snap = _snap(tf)
        assert_eq(_pct(snap), 80, "seek to 80%")
        fill_80 = _fill_w(snap)
        assert fill_80 > fill_40, f"a wider seek fills more ({fill_40} -> {fill_80})"
        assert _knob_x(snap) > knob_40, "the knob tracks the fill rightward"
        assert _state_word(snap) == "Paused", "still paused after a seek"

        # ── seek back left: fill shrinks ─────────────────────────────
        _seek(tf, 0.1)
        snap = _snap(tf)
        assert_eq(_pct(snap), 10, "seek back to 10%")
        assert _fill_w(snap) < fill_40, "the fill shrinks on a seek back left"

        # ── Play (toggle): Playing, the toggle relabels ──────────────
        tf.click(path=TOGGLE)
        snap = _snap(tf)
        assert _state_word(snap) == "Playing", f"toggle plays: {_status(snap)}"
        assert _toggle_label(snap) == "Pause", "a playing toggle offers Pause"

        # ── scene/tick advances the playhead (monotonic while playing) ─
        prev = _pct(_snap(tf))
        for step in range(3):
            tf.tick(1.0)
            snap = _snap(tf)
            now = _pct(snap)
            assert now >= prev, f"tick {step}: playhead did not go backwards ({prev} -> {now})"
            assert _state_word(snap) == "Playing", "still Playing while ticking"
            prev = now
        assert prev > 10, f"the playhead advanced past the 10% start: {prev}"

        # ── seek while playing: a big jump, yet still Playing ────────
        _seek(tf, 0.9)
        snap = _snap(tf)
        assert _state_word(snap) == "Playing", "a seek mid-play stays Playing (jump-and-continue)"
        assert _pct(snap) >= 88, f"the playhead jumped to ~90%: {_pct(snap)}"
        assert _pct(snap) > prev, f"the seek jumped forward from {prev}"

        # ── Pause (toggle): frozen across a tick, relabels to Play ───
        tf.click(path=TOGGLE)
        snap = _snap(tf)
        assert _state_word(snap) == "Paused", f"toggle pauses: {_status(snap)}"
        assert _toggle_label(snap) == "Play", "a paused toggle offers Play"
        frozen = _pct(snap)
        tf.tick(3.0)  # a paused clock is at rest — a no-op
        assert_eq(_pct(_snap(tf)), frozen, "a paused playhead is frozen across a tick")

        # ── seek to the end: 100%, Paused, a full bar ────────────────
        _seek(tf, 1.0)
        snap = _snap(tf)
        assert_eq(_pct(snap), 100, "seek to the end reads 100%")
        assert _state_word(snap) == "Paused", "seek-to-end is Paused"
        track_w = int(rect_of(_node(snap, TRACK))["w"])
        assert _fill_w(snap) >= track_w - 1, "a full seek fills the whole bar"

        # ── Play from the end: rewinds and replays ───────────────────
        tf.click(path=TOGGLE)
        snap = _snap(tf)
        assert _state_word(snap) == "Playing", "Play from the end -> Playing"
        assert _pct(snap) < 20, f"Play from a sought end rewinds to the start: {_pct(snap)}"


if __name__ == "__main__":
    sys.exit(run_demo("R1415 scrubber", body))
