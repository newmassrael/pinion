#!/usr/bin/env python3
"""R1620 §5.45 §5.35 §2 #2 — a drag-select reaches PAST the viewport.

R1619 made drag-select expressible. It could still only select what was on
screen: a row that is not painted is never entered, so a sweep stopped at the
last visible line. Auto-scroll is the other half — hold the button near an
edge and the view keeps moving, so the gesture reaches the rest of the model.
The reference calls it `autoScroll` on its abstract item view, and no view
without it can select past its own bottom edge.

Two things here are past the reference, and both were read from its source
rather than assumed:

* **Speed is a function of the POINTER.** There, a counter starts at zero when
  auto-scroll begins and increments once per timer tick (capped at the page
  step), and that counter IS the per-tick distance — so the speed depends only
  on how long the drag has lasted, and the pointer's depth into the margin is
  read as a boolean. Here the margin is a ramp: at its inner edge the speed is
  zero, at the viewport boundary it is the declared maximum, linearly between,
  saturating outside. Pushing further goes faster and easing back slows down.
* **Nothing is fabricated.** Scrolling moves content under a stationary cursor,
  so the address under the pointer changes with no input at all. The reference
  handles that by synthesising a mouse-move and posting it to the viewport —
  an event the application cannot tell from the user's. Here the new hover
  target is a derivation of the new picture, and this script checks the
  selection follows anyway.

And it is introspectable: `scene/input_state.auto_scroll` says what the ramp is
doing and what band would make it do something, so "why is my drag not
scrolling" is answerable rather than guessable.

Run from the workspace root:
    cargo build -p hello-multi-select --release
    python3 tools/demos/r1620_a_drag_reaches_past_the_edge.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    call,
    indexed_tags,
    run_demo,
)

EXAMPLE = "hello-multi-select"
LIST_TAG = "vlist"
SCROLL_TAG = "vlist_scroll"


def input_state(tf: RpcSubprocess) -> dict:
    return call(tf, "scene/input_state")


def painted_rows(tf: RpcSubprocess) -> list[int]:
    """Which data rows are on screen right now."""
    rects = abs_rects_of(tf.snapshot(source="paint"))
    return sorted(indexed_tags(rects, f"{LIST_TAG}#"))


def selection(tf: RpcSubprocess):
    return tf.query("/external/selection")


def edge_point(tf: RpcSubprocess, inset: float) -> tuple[float, float]:
    """A point `inset` px above the scroll viewport's bottom edge.

    Addressed by COORDINATE rather than by row tag, for two reasons. A user
    drags to a pixel, not to a row — the gesture under test is "hold near the
    edge", and which row happens to be there is exactly the thing that changes
    while it runs. And the last row of a windowed list is clipped in and out by
    a pixel as the view slides, so a tag address fails as a LOOKUP, which reads
    as the feature being broken rather than as the script pointing at the wrong
    place.
    """
    x, y, w, h = abs_rects_of(tf.snapshot(source="paint"))[SCROLL_TAG]
    return (x + w / 2.0, y + h - inset)


def middle_point(tf: RpcSubprocess) -> tuple[float, float]:
    """The centre of the scroll viewport — outside every edge band, so a press
    here opens a gesture without immediately asking the ramp for anything."""
    x, y, w, h = abs_rects_of(tf.snapshot(source="paint"))[SCROLL_TAG]
    return (x + w / 2.0, y + h / 2.0)


def run(tf: RpcSubprocess) -> None:
    # ── 0. the surfaces are discoverable ─────────────────────────────────
    catalogue = {m["name"] for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/input_state" in catalogue
    assert "scene/scroll_state" in catalogue

    state = input_state(tf)
    assert "auto_scroll" in state, (
        "the axis is always present in the reply; its VALUE says whether a "
        "gesture holds a scroll region"
    )
    assert_eq(
        state["auto_scroll"],
        None,
        "null at rest — no gesture holds a region, which is a different fact "
        "from a ramp that is holding one and reading zero",
    )

    visible = painted_rows(tf)
    assert len(visible) > 2, f"a windowed list is on screen: {visible}"
    top, bottom = visible[0], visible[-1]
    print(f"[demo] rows {top}..{bottom} are painted at boot")

    # ── 1. a sweep with no auto-scroll stops at what is painted ──────────
    #      Press the top visible row, drag to the last visible one.
    tf.pointer_button("left", "down", path=f"{LIST_TAG}#{top}")
    tf.hover(at=edge_point(tf, 3.0))
    swept_visible = selection(tf)
    assert swept_visible, f"the sweep selected something: {swept_visible}"
    held = input_state(tf)["held_pointer_buttons"]
    assert_eq(held, ["left"], "the gesture is open (R1619's fact)")

    # ── 2. the ramp is LIVE, and it says so ──────────────────────────────
    #      The pointer is over the last painted row, which sits in the bottom
    #      edge band, so the ramp is asking for downward velocity.
    ramp = input_state(tf)["auto_scroll"]
    assert ramp is not None, "a gesture holds the region, so the axis answers"
    assert ramp["margin"] > 0.0, f"the region declares a band: {ramp}"
    assert ramp["max_speed"] > 0.0, f"and a top speed: {ramp}"
    print(f"[demo] ramp: {ramp['velocity']} px/s in a {ramp['margin']}px band")

    # ── 3. frames pass with the pointer STILL, and the view moves ────────
    before_offset = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"]
    for _ in range(20):
        tf.tick(0.016)
    after_offset = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"]
    assert after_offset > before_offset, (
        f"the view auto-scrolled with no further input: {before_offset} -> "
        f"{after_offset}"
    )
    now_visible = painted_rows(tf)
    assert now_visible[-1] > bottom, (
        f"and rows that were off screen are now painted: {now_visible[-1]} > {bottom}"
    )
    print(f"[demo] scrolled {before_offset} -> {after_offset}; rows now {now_visible[0]}..{now_visible[-1]}")

    # ── 4. the SELECTION followed, with no synthetic input event ─────────
    grown = selection(tf)
    assert grown != swept_visible, (
        "the sweep grew past the rows that were painted when it began — the "
        f"whole point: {swept_visible} -> {grown}"
    )
    print(f"[demo] selection grew past the viewport: {swept_visible} -> {grown}")

    # ── 5. the release stops it, and the axis goes back to null ──────────
    tf.pointer_button("left", "up", at=edge_point(tf, 3.0))
    assert_eq(input_state(tf)["held_pointer_buttons"], [])
    assert_eq(
        input_state(tf)["auto_scroll"],
        None,
        "no gesture, no ramp — absent rather than a zeroed object",
    )
    settled = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"]
    for _ in range(20):
        tf.tick(0.016)
    assert_eq(
        call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"],
        settled,
        "and 20 more frames move nothing — a resting pointer near an edge is "
        "reading, not dragging",
    )
    kept = selection(tf)
    # The release's own cursor move is one more crossing with the button still
    # held, so the range may take in the row under the release point — that is
    # the sweep working, not the release resetting. What must hold is that the
    # range is still anchored where the finger went down and did not collapse.
    assert_eq(kept[0][0], 0, f"still anchored at the press: {kept}")
    assert kept[0][1] >= grown[0][1], (
        f"the release did not collapse the swept range: {grown} -> {kept}"
    )
    print("[demo] the release stops the ramp and keeps the range")

    # ── 6. NEGATIVE CONTROL: hovering the same edge scrolls nothing ──────
    #      Identical pointer position, identical ticks, no button held.
    base = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"]
    tf.hover(at=edge_point(tf, 3.0))
    for _ in range(20):
        tf.tick(0.016)
    assert_eq(
        call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"],
        base,
        "hovering an edge for 20 frames moves nothing — without this check the "
        "whole script would pass against a view that scrolls whenever the "
        "pointer is low",
    )
    assert_eq(input_state(tf)["auto_scroll"], None)
    print("[demo] hovering the same edge for the same frames moves nothing")

    # ── 7. the ramp reads the POINTER, not the clock ─────────────────────
    #      Two gestures over the same frames, differing only in how deep the
    #      pointer sits in the band. The reference cannot express this: its
    #      speed is a function of elapsed ticks alone.
    def travel(inset: float) -> int:
        start = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"]
        tf.pointer_button("left", "down", at=middle_point(tf))
        tf.hover(at=edge_point(tf, inset))
        for _ in range(10):
            tf.tick(0.016)
        moved = call(tf, "scene/scroll_state", {"tag": SCROLL_TAG})["offset"]["y"] - start
        tf.pointer_button("left", "up", at=edge_point(tf, inset))
        return moved

    # 12 px above the edge is just inside a 16-px band; 1 px is nearly at it.
    shallow = travel(12.0)
    deep = travel(1.0)
    print(f"[demo] ten frames travelled {shallow} px shallow vs {deep} px deep")
    assert deep >= shallow, (
        f"deeper in the band is not slower ({deep} vs {shallow}) — the speed is "
        "a function of the pointer's position, which is the axis the reference "
        "does not have at all"
    )

    assert deep > shallow * 1.5, (
        f"and MEASURABLY faster, not merely not-slower: {deep} vs {shallow} px "
        "over the same ten frames"
    )

    # ── 8. the ramp is signed, and it saturates ──────────────────────────
    #      Reading the published velocity at four depths on both edges is the
    #      whole model in one pass: still in the middle, signed by which edge,
    #      proportional inside the band, and capped at the declared maximum
    #      once the pointer reaches (or passes) the boundary.
    def velocity_at(point: tuple[float, float]) -> float:
        tf.hover(at=point)
        ramp_now = input_state(tf)["auto_scroll"]
        assert ramp_now is not None, "the gesture is still open"
        return float(ramp_now["velocity"]["y"])

    x, y, w, h = abs_rects_of(tf.snapshot(source="paint"))[SCROLL_TAG]
    tf.pointer_button("left", "down", at=middle_point(tf))
    top_speed = float(input_state(tf)["auto_scroll"]["max_speed"])
    assert_eq(velocity_at(middle_point(tf)), 0.0, "the middle is still")
    assert_eq(velocity_at((x + w / 2.0, y + h - 20.0)), 0.0, "outside the band")
    half_down = velocity_at((x + w / 2.0, y + h - 8.0))
    assert half_down > 0.0, f"inside the bottom band, downward: {half_down}"
    assert half_down < top_speed, f"and not yet at top speed: {half_down}"
    at_edge = velocity_at((x + w / 2.0, y + h - 0.5))
    assert at_edge > half_down, f"deeper is faster: {at_edge} > {half_down}"
    assert at_edge <= top_speed + 1e-6, f"capped at the declared max: {at_edge}"
    up_half = velocity_at((x + w / 2.0, y + 8.0))
    assert up_half < 0.0, f"the top band is the mirror image, upward: {up_half}"
    assert abs(abs(up_half) - half_down) < 1e-6, (
        f"and symmetric: {up_half} vs {half_down}"
    )
    tf.pointer_button("left", "up", at=middle_point(tf))
    assert_eq(input_state(tf)["auto_scroll"], None)
    print(
        f"[demo] ramp over the band: 0 -> {half_down} -> {at_edge} "
        f"(max {top_speed}), and {up_half} at the other edge"
    )

    print("[demo] a drag reaches past the edge")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1620 §5.45 §5.35 — a drag reaches past the edge", body)
