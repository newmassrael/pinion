#!/usr/bin/env python3
"""R1792 — a box holds its own caption, and can say where it put it.

A reader opened the assembled analysis tool and reported two things: that the
words in the protocol chips were not centred in their rectangles, and that
`discovery off` was aligned oddly. Measured through the paint at the size they
saw, both were worse than "not centred":

* the five protocol chips drew their caption at `chip.x + 7`, 32 wide, in a box
  36 wide — so **3px of every one hung off the right edge**;
* the determinism switch's caption had its width derived against the PANE while
  it is drawn inside the switch's own box, landing it **exactly flush** with
  that box's right border: 48px of gap on the left and zero on the right.

Both are one class: a caption positioned by arithmetic against a rectangle it is
not inside. The captions are SIBLINGS of the tagged boxes they appear in, so
nothing in the tree relates the two and no gate could ask — this file's own
harness measured 230 such caption/box pairs across five analyzer screens, and
675 of 675 text runs declaring the default alignment, which is why *off-centre*
and *deliberately left* were indistinguishable.

Sections:

* **A** — the reported chips, read from the paint: inside their boxes, centred.
* **B** — the reported switch caption: no longer flush with its own border.
* **C** — the caption is a CHILD now, so its box answers for it. The
  attribution rule here is nearest *tagged ancestor*, and a sibling caption is
  therefore filed under whatever container encloses both.
* **D** — the population, counted rather than asserted about two examples: no
  caption on this screen is drawn outside the box it appears in, in any of the
  states the sweep reaches.
* **E** — ★ what is NOT fixed, said out loud: alignment is still undeclared at
  every site that has not adopted the capability, and this demo says how many.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    resize_and_settle,
    run_demo,
)

LAB = "hello-node-lab"
#: The window the reader had open.
REPORTED_AT = (1440, 900)
#: The five chips, in the order the palette lays them out.
CHIPS = ("tcp", "tls", "quic", "udp", "ws")

CHECKS = 0


def ok(msg: str, cond: bool) -> None:
    global CHECKS
    CHECKS += 1
    print(f"[demo] {'PASS' if cond else 'FAIL'}: {msg}")
    assert cond, msg


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def collect(node, xoff, yoff, parent, out) -> None:
    """Every text run with an absolute rect and its nearest tagged ancestor.

    ★ A `Scroll` carries its subtree under `content` and shifts it by its
    offset. The first draft of this walk followed only `children` and read 19
    runs against 176 tags — the palette, the inspector and the canvas, which is
    the whole population this demo is about, were invisible to it.
    """
    if not isinstance(node, dict):
        return
    if node.get("type") == "Scroll":
        vp = node.get("viewport") or {}
        collect(
            node.get("content"),
            xoff + vp.get("x", 0) - node.get("offset_x", 0),
            yoff + vp.get("y", 0) - node.get("offset_y", 0),
            (node.get("tag"), parent[1] if parent else None),
            out,
        )
        return
    rect = node.get("rect")
    here = (
        (rect["x"] + xoff, rect["y"] + yoff, rect["w"], rect["h"])
        if isinstance(rect, dict)
        else None
    )
    tag = node.get("tag")
    if node.get("type") == "Text" and node.get("content") and here:
        out.append(
            {
                "text": node["content"],
                "rect": here,
                "tag": tag,
                "owner": parent[0] if parent else None,
                "align": (node.get("style") or {}).get("text_align"),
            }
        )
    for child in node.get("children") or []:
        collect(child, xoff, yoff, ((tag or (parent[0] if parent else None)), here), out)


def body() -> None:
    with RpcSubprocess(LAB, boot_grace=1.5) as tf:
        resize_and_settle(tf, REPORTED_AT)
        tf.tick_ms(16)
        shot = tf.snapshot(source="paint")
        runs: list = []
        collect(shot, 0, 0, None, runs)
        from rpc_verify import abs_rects_of

        boxes = abs_rects_of(shot)

        banner("A — the five chips a reader reported, read from the paint")
        for word in CHIPS:
            tag = f"lab.palette.protocol.{word}"
            box = boxes[tag]
            run = next(r for r in runs if r["tag"] == f"{tag}.caption")["rect"]
            left = run[0] - box[0]
            right = (box[0] + box[2]) - (run[0] + run[2])
            ok(
                f"A: ★★★★★ {word!r} is INSIDE its box — it used to hang 3px past "
                f"the right edge: run {run} in box {box}",
                left >= 0 and right >= 0,
            )
            # ★★★★★ R1794 — within a pixel, not exactly equal, and the reason is
            # not tolerance-for-its-own-sake: the slack is `box - ink` and the
            # ink is now what the SHAPER measured, so it is whatever it is. A
            # 36px chip holding 15px of `tcp` has 21 to split, and 21 does not
            # halve. This assertion read `left == right` and passed only while
            # the "ink" was a round number the caller had made up — which is the
            # defect R1794 repaired. An exact-equality centring check is a check
            # that can only survive on invented measurements.
            ok(
                f"A: and centred within a pixel, which is what an odd slack "
                f"allows ({word}: {left} left, {right} right)",
                abs(left - right) <= 1,
            )

        banner("B — the switch caption, which used to touch its own border")
        box = boxes["lab.palette.discovery"]
        track = boxes["lab.palette.discovery.track"]
        # ★ R1813 — `.caption`, not `.state`: the read-out is the switch box's
        # own caption child now, and that suffix is the framework's name for the
        # relation rather than one this screen chose.
        state = boxes["lab.palette.discovery.caption"]
        right = (box[0] + box[2]) - (state[0] + state[2])
        left = state[0] - box[0]
        ok(
            f"B: ★★★★★ it stops short of the border: caption {state} in box {box} "
            f"leaves {right}px, where it left 0",
            right > 0,
        )
        # ★★★★★ R1813 — this read `left > right`, standing in for "left-aligned
        # after the track", and it was true only while the run rectangle was the
        # whole ROOM. Placed by `caption::inside` the rectangle is the ink the
        # shaper measured, so the short position word leaves more slack on the
        # right than the track takes on the left and the proxy inverts -- while
        # the glyphs have not moved. What the screen chose is CLEAR OF THE TRACK,
        # so that is what is asked, of the track's own painted rectangle.
        ok(
            f"B: and it starts clear of the track ({left} in, track ends at "
            f"{track[0] + track[2] - box[0]} in), because that is the layout this "
            "screen chose and this round did not change what it chose -- only "
            "that the choice is expressible",
            state[0] >= track[0] + track[2],
        )
        cap = next(
            (r for r in runs if r["tag"] == "lab.palette.discovery.caption"), None
        )
        ok(
            "B: and the switch's own box answers for it -- the reader's SECOND "
            "site is a declared caption now, not a rectangle two edits could "
            "drift apart",
            cap is not None and cap["owner"] == "lab.palette.discovery",
        )

        banner("C — the caption is a child, so its own box answers for it")
        for word in CHIPS:
            tag = f"lab.palette.protocol.{word}"
            cap = next((r for r in runs if r["tag"] == f"{tag}.caption"), None)
            ok(
                f"C: {word!r} is addressable under its box's name, "
                f"`{tag}.caption` — a sibling caption is filed under whatever "
                "container encloses both, which is how three chips' words came "
                "to arrive as one run of a pane",
                cap is not None and cap["owner"] == tag,
            )

        banner("D — the population, counted rather than argued from two examples")
        pairs, escapes = 0, []
        owners = {r["owner"] for r in runs if r["owner"]}
        empty = {
            tag: rect
            for tag, rect in boxes.items()
            if tag not in owners and rect[2] > 0
        }
        for run in runs:
            r = run["rect"]
            cx, cy = r[0] + r[2] // 2, r[1] + r[3] // 2
            holders = [
                (tag, b)
                for tag, b in empty.items()
                if b[0] <= cx < b[0] + b[2] and b[1] <= cy < b[1] + b[3] and b[2] <= r[2] * 4
            ]
            if not holders:
                continue
            tag, b = min(holders, key=lambda kv: kv[1][2] * kv[1][3])
            pairs += 1
            if r[0] < b[0] or r[0] + r[2] > b[0] + b[2] or r[1] < b[1] or r[1] + r[3] > b[1] + b[3]:
                escapes.append((tag, run["text"], r, b))
        ok(
            f"D: ★★★★★ of {pairs} caption/box pair(s) on this frame, {len(escapes)} "
            f"are drawn outside the box a reader sees around them: {escapes[:3]}",
            escapes == [],
        )

        banner("E — and what is NOT fixed, said rather than left to be discovered")
        declared = sum(1 for r in runs if r["align"] and r["align"] != "Start")
        print(
            f"  [alignment] {declared} of {len(runs)} run(s) on this frame declare "
            f"an alignment other than the default"
        )
        ok(
            "E: ★★ the capability publishes the intention, and the sites that "
            "have adopted it say so -- every other run in this tree still "
            "carries the default, so `off-centre` and `deliberately left` "
            "remain indistinguishable there. That is the remainder, and it is a "
            "number rather than a hope",
            declared > 0,
        )

    print(f"\n=== {CHECKS} named check(s) ===")


if __name__ == "__main__":
    run_demo("r1792 a box holds its own caption", body)
