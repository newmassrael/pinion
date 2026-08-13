#!/usr/bin/env python3
"""R1656 §5.12 §5.32 §5.36 §2 #7 — **what the frame painted, against the box it
was promised.**

A rectangle in a scene is a promise. Every read in this tree reported the
promise; nothing reported whether it was kept. So the analysis-tool screen could
paint seven of its eight node cards' last field row three to five pixels below
their own border, at the size it opens in, and describe itself as correct to
every gate, every test and every agent — until a person looked at it and said
so.

This drives the two capabilities that close that, over the wire, on the real
pipeline:

* `scene/containment` — every painted mark that left the box that owns it, per
  edge, with the string a reader is losing and whether a clip cut it away.
* `External::on_resize` — a §5.15 lifecycle arm that had been **declared since
  the contract was written and never called**, which is why a consumer that
  wanted pixels out of `pointer_move`'s fraction had to multiply by a constant,
  and why every press after a maximise landed somewhere else.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1656_a_mark_says_the_box_it_left.py
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
    resize_and_settle,
    run_demo,
    walk_nodes,
)

EXT = "/external"
WIN = (1440, 900)
BIG = (2494, 1531)


def containment(tf):
    return call(tf, "scene/containment")


def text_runs(tf):
    return call(tf, "scene/text_painted")["runs"]


def run(tf: RpcSubprocess) -> None:
    # ── 1. the read exists, is shaped, and answers about the whole screen ─────
    out = containment(tf)
    for key in ("escapes", "smeared", "clipped", "marks"):
        assert key in out, f"scene/containment must report {key}: {out.keys()}"
    assert out["marks"] > 500, (
        f"only {out['marks']} mark(s) examined — the count is beside the list so "
        f"an empty list on a surface that painted nothing cannot read as coverage"
    )
    assert_eq(out["smeared"] + out["clipped"], len(out["escapes"]),
              "every escape is one of the two fates")

    # ── 2. the screen this round repaired is contained ────────────────────────
    assert_eq(len(out["escapes"]), 0,
              "the node graph lab paints nothing outside the box that owns it")

    # ── 3. and the check is capable of failing: drive the screen into states
    #      the specification does not describe and ask again. R1652.1's lesson —
    #      the sweep that only visits the opening screen visits the one state
    #      nobody works in.
    tf.invoke(f"{EXT}/select", "R-01")
    for _ in range(6):
        # Through the affordance the screen offers, not through a back door: a
        # state a demo invents can be one the screen cannot reach.
        tf.click(path="lab.form.item.listen.endpoints.add")
    tf.invoke(f"{EXT}/set_field", "id=a-considerably-longer-identifier-than-fits")
    grown = containment(tf)
    assert grown["marks"] >= out["marks"], (
        f"the grown screen paints at least as much: {grown['marks']} vs {out['marks']}"
    )
    sideways = [e for e in grown["escapes"] if e["over"]["left"] or e["over"]["right"]]
    assert_eq(len(sideways), 0,
              "nothing is painted over the pane beside it, however long the data")

    # ── 4. an escape names what a reader loses, not just that one happened ────
    #      Asserted on the SHAPE of a row rather than by manufacturing one: the
    #      screen is clean, and a demo that dirtied it to read a row would be
    #      asserting against its own edit.
    zoomed = call(tf, "scene/containment")
    for e in zoomed["escapes"]:
        for key in ("tag", "path", "owner", "content", "promised", "painted",
                    "owner_rect", "over", "fate"):
            assert key in e, f"an escape row must carry {key}: {e}"
        assert e["fate"] in ("smeared", "clipped"), e["fate"]

    # ── 5. the discriminating numbers on the text read ───────────────────────
    #      `overflows` is true of most runs on most screens (a shaped line box is
    #      taller than the face it holds), so the amount and the axis are what a
    #      caller can threshold on.
    runs = text_runs(tf)
    assert len(runs) > 40, f"the screen has text: {len(runs)}"
    for r in runs:
        assert "over_w" in r and "over_h" in r, f"the amount, per axis: {r}"
        assert_eq(
            r["overflows"],
            r["over_w"] > 0 or r["over_h"] > 0,
            f"the boolean is derived from the numbers beside it: {r['content']!r}",
        )
    discriminating = [r for r in runs if r["over_w"] > 0]
    assert len(discriminating) < len(runs) // 2, (
        f"{len(discriminating)} of {len(runs)} runs are too WIDE for their box — "
        f"the horizontal axis is the one an author gets wrong, and it should be "
        f"rare; the vertical one is near-universal and is why the boolean alone "
        f"cannot gate"
    )

    # ── 6. §5.15 — the surface is told its size, and the fraction follows it ──
    before = tf.query(f"{EXT}/cursor")
    tf.hover(at=(700, 400))
    at_open = tf.query(f"{EXT}/cursor")
    assert_eq(at_open, "700,400",
              "at the opening size the app is told the cursor it was sent")

    # ★ R1686 — settled rather than ticked. A fixed tick after a resize races
    # the render, and the assertion below would then be reading the OPENING
    # window's root rect: a demo that is green only when the machine is quiet.
    root = resize_and_settle(tf, BIG)["rect"]
    assert_eq((root["w"], root["h"]), BIG, "the window really grew")

    # The same window coordinate, after the growth. Before R1656 this arrived
    # scaled by opening-size over current-size — measured 0.5775x horizontally —
    # because `pointer_move` hands a FRACTION and the basis was a constant.
    tf.hover(at=(700, 400))
    at_big = tf.query(f"{EXT}/cursor")
    assert_eq(at_big, "700,400",
              "and after a maximise it is told the same coordinate, because "
              "`External::on_resize` now tells it what the fraction is OF")

    # ── 7. a press at the far side of a grown window reaches the node under it,
    #      which is the failure a person reported.
    by_tag = {r["tag"]: r for r in text_runs(tf) if r.get("tag")}
    ids = [t for t in by_tag if t.startswith("lab.node.") and t.endswith(".id")]
    assert len(ids) >= 6, f"the graph paints its cards: {ids}"

    # ★★ R1682.1 — the picked link's own chrome is a summoned OVERLAY, and an
    # overlay covering part of what is under it is what it is for. Measured on
    # the behaviour reference: its endpoint seats are a flex row laid on the
    # wire's midpoint with no avoidance of anything, so this is agreement with
    # it rather than a concession. The screen's in-process card sweeps have
    # taken the same exception since R1681.3; this one did not, and phase 3
    # above is what makes the difference visible — it grows the picked link's
    # target to seven addresses, so the seat row spans far enough to cover a
    # card two columns away.
    #
    # Derived from the PAINT, never restated: the seats carry their own tags, so
    # "is this aim point under the chrome" has one source and cannot drift from
    # where the chrome actually is.
    chrome = [
        rect
        for tag, rect in abs_rects_of(tf.snapshot(source="paint")).items()
        if tag.startswith("lab.link.")
    ]

    def under_chrome(px, py):
        return any(
            x <= px < x + w and y <= py < y + h for (x, y, w, h) in chrome
        )

    reached = 0
    covered = 0
    for tag in sorted(ids):
        node = tag[len("lab.node."):-len(".id")]
        other = "Q-01" if node != "Q-01" else "T-01"
        tf.invoke(f"{EXT}/select", other)
        r = by_tag[tag]
        at = (r["x"] + r["w"] // 2, r["y"] + r["h"] // 2)
        if under_chrome(*at):
            covered += 1
            continue
        tf.click(at=at)
        assert_eq(tf.query(f"{EXT}/selected"), node,
                  f"{node} answers a press where it is painted, on a grown window")
        reached += 1
    # The exception is BOUNDED and the bound is asserted: an overlay that
    # covered most of the graph would pass a check that merely skipped what it
    # covers, which is the way this sort of excuse goes wrong.
    assert covered <= 1, (
        f"the picked link's chrome covers {covered} of {len(ids)} cards' "
        f"identity lines — an overlay is allowed to cover something, not most "
        f"of the canvas"
    )
    assert reached >= 6, reached

    # ── 8. the containment answer survives the resize: a screen that reflowed
    #      has to be re-judged, not judged once at boot.
    after = containment(tf)
    assert_eq(len(after["escapes"]), 0,
              "and it is still contained at the grown size")
    assert after["marks"] > 500, after["marks"]

    # ── 9. an unavailable answer is not a clean one. A host that cannot shape
    #      must say so rather than report an empty list.
    assert "escapes" in after, "the shaped host answers"
    assert isinstance(after["escapes"], list), type(after["escapes"])

    print(f"  containment: {after['marks']} marks examined, 0 escapes, "
          f"before {before!r} -> {at_big!r} after a maximise, {reached} cards "
          f"reachable by position on a {BIG[0]}x{BIG[1]} window")


def main() -> None:
    with RpcSubprocess("hello-node-lab") as tf:
        run(tf)


if __name__ == "__main__":
    run_demo("hello-node-lab", main)
