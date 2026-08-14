#!/usr/bin/env python3
"""R1662 §5.12 §5.45 §5.32 §2 #7 — **what the screen is not showing, and whether
the reader can get to it.**

Two different things stop a mark being on screen, and until this round they were
the same answer:

* the pane it lives in has scrolled past it — the reader scrolls back, nothing
  is wrong;
* nothing can bring it into view at all — the pane does not scroll, or its
  content extent stops short of where the mark was placed.

`scene/containment` cannot tell them apart, by construction: it asks whether a
mark stayed inside the box that owns it, and a scroll viewport is *supposed* to
be smaller than its content. So two of the analysis-tool screens shipped with
controls a person could never see — one below a palette that did not scroll, one
below a board that did not scroll — with every ink gate green.

This drives the capability that closes it, on the real pipeline:

* `scene/scroll_reach` — every mark that is off screen, the viewport it was
  judged against (its size, its content extent, its range, whether it fits), and
  either the offset that reveals it or how far past the reachable box it sits.
* and then it **scrolls to that offset over the wire and presses the control**,
  because an offset nobody has driven is arithmetic rather than a capability.

Past the reference toolkit: there a scroll area derives its range from the
laid-out child (parity, and this tree has done that since R55.C), but the
per-mark question has no surface at all — overflow can only be inferred from
`maximum() > 0`, which is also the answer an area that never set its range
gives. A screen whose panes do not scroll is, from outside the widget,
indistinguishable from one whose content fits.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1662_a_pane_says_what_no_scrolling_can_reach.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    resize_and_settle,
    run_demo,
)

EXT = "/external"
#: The size this demo judges at: the screen's own floor plus a margin.
#:
#: ★★★ R1689 — DERIVED, and this is the second time the same class bit this
#: file. R1687 read `FLOOR` from the screen because a copied floor goes stale
#: the moment a button is added to the toolbar — and left `WIN` written down as
#: `1500`, which is the same fact ("a size above the floor") in the same file.
#: R1689 added three buttons, the floor passed 1500, and the guard below fired
#: on a demo that was not wrong about anything. A constant that has to stay in a
#: RELATION with a moving number is not a constant.
WIN: tuple[int, int] = (0, 0)  # derived from the floor at start-up
#: How far above the floor this demo judges — the relation, which is what was
#: actually meant. 58 px is what the pair had when this demo was written.
ABOVE_FLOOR = 58
#: `MIN_W` x `MIN_H` as the screen derives them: the rail, the palette, the
#: toolbar's two clusters and the inspector across; the two bars plus what the
#: canvas chrome needs down. Below this the screen DECLARES it cannot paint, and
#: `scene/resize` will happily go there — the first draft of this demo used 1084
#: and read back twelve lost marks that are simply a screen asked for a size it
#: says it does not support.
#:
#: ★★★ R1687 — READ from the screen, not written here. It was `(1316, 360)`, a
#: second copy of a number the screen derives; R1687 moved the floor to 1442 by
#: putting one more button in the toolbar and this copy stayed behind, so the
#: demo failed against a fact about the screen instead of a defect. The
#: specification publishes it now, for the same reason it publishes the
#: operations table.
FLOOR: tuple[int, int] = (0, 0)  # filled from the screen at start-up


def reach(tf):
    return call(tf, "scene/scroll_reach")


def run(tf: RpcSubprocess) -> None:
    # ★★★ R1687 — the floor comes from the screen. See `FLOOR`.
    global FLOOR, WIN
    declared = json.loads(tf.query(f"{EXT}/spec"))["floor"]
    FLOOR = (declared[0], declared[1])
    WIN = (FLOOR[0] + ABOVE_FLOOR, max(900, FLOOR[1] + ABOVE_FLOOR))
    assert FLOOR[0] < WIN[0] and FLOOR[1] < WIN[1], (
        f"this demo judges at {WIN} and the screen's floor is {FLOOR} — a "
        "window below the floor is clamped, and every read below would be "
        "about a size the screen says it does not support"
    )
    # ★ ASKED FOR, not assumed. The size the harness happens to open at was
    # above the floor until R1687 raised it, so this demo read the boot window
    # and called it `WIN`; when the floor passed it, the clamp made every read
    # here about a size nobody had chosen.
    resize_and_settle(tf, WIN)

    # ── 1. the read exists and describes what it judged against ──────────────
    out = reach(tf)
    for key in ("window", "marks", "scrollable", "lost", "out_of_sight"):
        assert key in out, f"scene/scroll_reach must report {key}: {out.keys()}"
    assert out["marks"] > 500, (
        f"only {out['marks']} mark(s) examined — the count rides beside the list "
        f"so an empty list on a surface that painted nothing cannot read as "
        f"coverage"
    )
    assert_eq(out["window"]["w"], WIN[0], "the window it judged against")
    assert_eq(out["window"]["h"], WIN[1], "and its height")
    assert_eq(
        out["scrollable"] + out["lost"],
        len(out["out_of_sight"]),
        "every out-of-sight mark is one of the two verdicts",
    )

    # ── 2. nothing on this screen is unreachable ─────────────────────────────
    assert_eq(len(list(o for o in out["out_of_sight"] if o["reach"] == "lost")), 0,
              "the node graph lab paints nothing no gesture can reach")

    # ── 3. a row names its viewport by tag, in full ──────────────────────────
    #      Asserted on the SHAPE of every row rather than on one manufactured
    #      case: the numbers are what a repair needs, and a row that carried
    #      only a boolean would be a row nobody can act on.
    for o in out["out_of_sight"]:
        for key in ("tag", "path", "content", "rect", "viewport", "reach"):
            assert key in o, f"an out-of-sight row must carry {key}: {o}"
        assert o["reach"] in ("scrollable", "lost"), o["reach"]
        v = o["viewport"]
        for key in ("name", "w", "h", "content_w", "content_h",
                    "at_x", "at_y", "max_x", "max_y", "fits"):
            assert key in v, f"a viewport must publish {key}: {v}"
        assert_eq(
            v["fits"],
            v["max_x"] == 0 and v["max_y"] == 0,
            f"`fits` is derived from the range beside it: {v['name']}",
        )
        if o["reach"] == "scrollable":
            assert o["to_x"] is not None and o["to_y"] is not None, o
            assert o["short_by"] is None, "a reachable mark is short by nothing"
            assert 0 <= o["to_y"] <= v["max_y"], f"the offset is in range: {o}"
        else:
            assert o["short_by"] is not None and o["to_y"] is None, o

    # The two side panes the specification declares as scrolling bodies. That
    # they ARE scroll containers is asserted in step 6, where the window is
    # small enough for them to be holding content off screen — asserting it
    # here, at the opening size, would be asserting about a screen that has
    # nothing to scroll.
    bodies = {"lab.palette.body", "lab.inspector.body"}

    # ── 5. shrink the window to the floor it declares. THIS is the state the
    #      round is about: the panes now hold more than they show, and every
    #      control has to remain reachable rather than merely painted.
    # ★ A resize is not a repaint. These reads answer from the frame the WINDOW
    # last painted (that is the point — introspection from the paint, not from a
    # re-render nobody saw), so the frame has to land before the question is
    # meaningful. Without this the read answered with the old layout's
    # rectangles inside the new window and reported twelve marks lost.
    # ★★ R1686 — and the wait is on the OUTCOME now. This round found the same
    # shape flaking in a sibling demo under load: a fixed tick is a bet on the
    # render arriving, and it is the bet this comment already knew was there.
    resize_and_settle(tf, FLOOR)
    small = reach(tf)
    assert_eq(small["window"]["w"], FLOOR[0], "the read follows the window")
    assert_eq(small["window"]["h"], FLOOR[1], "and its height")
    lost = [o for o in small["out_of_sight"] if o["reach"] == "lost"]
    assert_eq(len(lost), 0,
              f"nothing became unreachable at the declared floor: {lost[:3]}")
    away = [o for o in small["out_of_sight"] if o["reach"] == "scrollable"]
    assert len(away) > 10, (
        f"only {len(away)} mark(s) are off screen at {FLOOR[0]}x{FLOOR[1]} — the "
        f"floor is supposed to be the size where the side panes overflow, so a "
        f"small number here means this step is checking nothing"
    )

    # ── 6. the pane says its content is bigger than itself, as numbers ───────
    panes = {o["viewport"]["name"]: o["viewport"] for o in away}
    assert bodies <= set(panes), (
        f"both side panes should be holding content off screen: {sorted(panes)}"
    )
    for name in sorted(bodies):
        v = panes[name]
        assert not v["fits"], f"{name} says it fits and it does not: {v}"
        assert v["content_h"] > v["h"], (
            f"{name} content {v['content_h']} vs viewport {v['h']}"
        )
        assert_eq(
            v["max_y"],
            v["content_h"] - v["h"],
            f"{name}'s range is its content minus its viewport",
        )

    # ── 7. drive the published offset and the mark arrives ───────────────────
    #      The whole point: an offset that has never been scrolled to is a
    #      claim, not a capability.
    target = next(
        o for o in away
        if o["viewport"]["name"] == "lab.palette.body" and o["tag"]
        and o["tag"].startswith("lab.palette.role.")
    )
    tag, to_y = target["tag"], target["to_y"]
    assert to_y > 0, f"the target is below the fold: {target}"
    tf.scroll("lab.palette.body", to=(0, to_y))
    tf.tick(0.05)
    after = reach(tf)
    still_away = {o["tag"] for o in after["out_of_sight"] if o["tag"]}
    assert tag not in still_away, (
        f"{tag} was published as reachable at y={to_y} and scrolling there left "
        f"it off screen"
    )

    # ── 8. and it presses, at the place it landed ────────────────────────────
    #      A press on a palette role adds a node, so the screen has to paint
    #      MORE than it did — which is the observable that says the press
    #      arrived, rather than that the call returned.
    before_marks = after["marks"]
    tf.click(path=tag)
    tf.tick(0.05)
    pressed = reach(tf)
    assert pressed["marks"] > before_marks, (
        f"pressing {tag} at the place the published offset put it added no "
        f"node: {pressed['marks']} marks vs {before_marks}"
    )

    # ── 9. the offset the report published is the LEAST move, not any move ───
    #      Measured against the reference's `ensureVisible`, which for a point
    #      900 into a 198-tall viewport answers 702 and not 900.
    tf.scroll("lab.palette.body", to=(0, 0))
    tf.tick(0.05)
    back = reach(tf)
    again = next(
        (o for o in back["out_of_sight"] if o["tag"] == tag), None
    )
    assert again is not None, f"{tag} is off screen again"
    v = again["viewport"]
    assert_eq(again["to_y"], to_y, "the same offset, from the same place")
    assert again["to_y"] <= again["rect"]["y"], (
        f"the offset never scrolls past the mark's own top: {again}"
    )
    assert again["to_y"] >= again["rect"]["y"] + again["rect"]["h"] - v["h"], (
        f"and it moves far enough to end the mark inside the viewport: {again}"
    )

    # ── 10. back at the opening size, the screen is clean again ─────────────
    resize_and_settle(tf, WIN)
    end = reach(tf)
    assert_eq(end["window"]["h"], WIN[1], "the read follows the window back")
    assert_eq(len([o for o in end["out_of_sight"] if o["reach"] == "lost"]), 0,
              "and nothing is unreachable at the size it opens in")

    print(
        f"  scroll-reach: {end['marks']} marks examined; at "
        f"{FLOOR[0]}x{FLOOR[1]} {len(away)} mark(s) were one scroll away and 0 "
        f"were lost; {tag} published y={to_y} and arrived there"
    )


def main() -> None:
    with RpcSubprocess("hello-node-lab") as tf:
        run(tf)


if __name__ == "__main__":
    run_demo("hello-node-lab", main)
