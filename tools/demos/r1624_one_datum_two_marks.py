#!/usr/bin/env python3
"""R1624 §5.28 §2 #7 — one datum, two marks.

A candlestick and an open-high-low-close bar are two renderings of the same
four prices. The reference toolkit ships a candlestick series and no bar one
at all, so there is nothing there for a bar to be a second series *of*; here
it is a `SessionMark` on the chart that already exists, which is what keeps
the sort, both x-axis readings, the log value axis and its off-scale report,
the window, the inspect readout and the published direction contrast shared
rather than reimplemented.

What each check discriminates:

* **The glyph changes and the chart does not.** Session centres must be
  identical under both marks — a bar that re-derived its own x placement would
  drift, and a picture comparison would not say so.
* **A bar is three nodes, and they are named.** Spine, open tick, close tick,
  each addressable, so a client can ask "where is Wednesday's close" instead
  of reading pixels.
* **Direction survives the loss of hue BY SHAPE.** With the `no hue` chip on,
  every colour is one ink, and the only thing left saying which way a session
  went is that the close tick is the higher one on a rise. This is asserted
  against the painted geometry with the colours equal, so a check that passed
  on colour cannot pass here.
* **The caption reports the chart it drew.** The status line is read back, not
  restated.

Run from the workspace root:
    cargo build -p hello-candlestick --release
    python3 tools/demos/r1624_one_datum_two_marks.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    indexed_tags,
    run_demo,
    text_of_tag,
)

EXAMPLE = "hello-candlestick"
WIN = (980, 640)
SESSIONS = 6


def centres(rects: dict, prefix: str) -> list[float]:
    out = []
    for i in indexed_tags(rects, prefix):
        x, _y, w, _h = rects[f"{prefix}{i}"]
        out.append(x + w / 2.0)
    return out


def run(tf: RpcSubprocess) -> None:
    snap = tf.snapshot(source="paint", viewport=WIN)
    rects = abs_rects_of(snap)

    # ── 1. the chart boots on candles ───────────────────────────────────
    candles = indexed_tags(rects, "chart.candle.")
    assert_eq(len(candles), SESSIONS, "one candle body per session")
    assert_eq(indexed_tags(rects, "chart.ohlc."), [], "and no bars yet")
    candle_centres = centres(rects, "chart.candle.")
    print(f"[demo] candles: {candles}")

    # ── 2. the bars chip swaps the mark ─────────────────────────────────
    tf.click(path="bar")
    tf.tick(0.016)
    bars = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert_eq(indexed_tags(bars, "chart.candle."), [], "the candles are gone")
    for i in range(SESSIONS):
        for part in ("range", "open", "close"):
            tag = f"chart.ohlc.{i}.{part}"
            assert tag in bars, f"{tag} is painted: {sorted(bars)[:12]}"
    print(f"[demo] bars painted for {SESSIONS} sessions, 3 nodes each")

    # ── 3. the mark changed the glyph and NOTHING else ──────────────────
    #      A bar deriving its own x placement is the plausible bug, and it
    #      would look almost right.
    bar_centres = []
    for i in range(SESSIONS):
        x, _y, w, _h = bars[f"chart.ohlc.{i}.range"]
        bar_centres.append(x + w / 2.0)
    for i, (a, b) in enumerate(zip(candle_centres, bar_centres)):
        assert abs(a - b) <= 1.0, f"session {i} moved: {a} vs {b}"
    print("[demo] session centres are identical under both marks")

    # ── 4. the spine spans more than the body did ───────────────────────
    #      The range is high-to-low; the body was open-to-close, which is
    #      inside it. A bar that drew only the body would pass check 2.
    taller = 0
    for i in range(SESSIONS):
        _x, _y, _w, spine_h = bars[f"chart.ohlc.{i}.range"]
        _x, _y, _w, body_h = rects[f"chart.candle.{i}"]
        assert spine_h >= body_h, f"session {i}: spine {spine_h} < body {body_h}"
        if spine_h > body_h:
            taller += 1
    assert taller >= SESSIONS - 1, f"the range exceeds the body: {taller}/{SESSIONS}"
    print(f"[demo] the spine is the RANGE, taller than the body in {taller} sessions")

    # ── 5. the two ticks sit on opposite sides of the spine ─────────────
    #      Compared at their MIDPOINTS: a stroked node's rect is inflated by
    #      half the stroke on each side, so an edge comparison would be a
    #      statement about the pen rather than about the mark.
    for i in range(SESSIONS):
        cx = bar_centres[i]
        ox, _oy, ow, _oh = bars[f"chart.ohlc.{i}.open"]
        clx, _cy, cw, _ch = bars[f"chart.ohlc.{i}.close"]
        assert ox + ow / 2.0 < cx, f"session {i}: the open tick is on the left"
        assert clx + cw / 2.0 > cx, f"session {i}: the close tick is on the right"
    print("[demo] open left, close right — the mark's own way of saying which")

    # ── 6. NEGATIVE CONTROL: with the hue stripped, SHAPE still answers ──
    tf.click(path="mono")
    tf.tick(0.016)
    mono = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    rising = falling = 0
    for i in range(SESSIONS):
        _x, oy, _w, _h = mono[f"chart.ohlc.{i}.open"]
        _x, cy, _w, _h = mono[f"chart.ohlc.{i}.close"]
        if cy < oy:
            rising += 1
        elif cy > oy:
            falling += 1
    assert rising > 0 and falling > 0, (
        "with one ink for every direction, the ticks are the only channel left "
        f"and both directions must still be legible: up {rising} down {falling}"
    )
    print(f"[demo] hue removed — up {rising}, down {falling}, read from geometry")

    # ── 7. the caption reports what it drew ─────────────────────────────
    said = text_of_tag(tf, "caption", viewport=WIN)
    assert "doji" in said, f"the caption names the doji: {said}"
    assert "contrast" in said, f"and publishes the direction contrast: {said}"
    print(f"[demo] caption: {said[:80]}...")

    # ── 8. and the mark comes back ──────────────────────────────────────
    tf.click(path="bar")
    tf.tick(0.016)
    back = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert_eq(len(indexed_tags(back, "chart.candle.")), SESSIONS, "candles again")
    assert_eq(indexed_tags(back, "chart.ohlc."), [], "and no bars")
    for i in range(SESSIONS):
        x, _y, w, _h = back[f"chart.candle.{i}"]
        assert abs((x + w / 2.0) - candle_centres[i]) <= 1.0, f"session {i} is home"

    print("[demo] one datum, two marks")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1624 §5.28 — one datum, two marks", body)
