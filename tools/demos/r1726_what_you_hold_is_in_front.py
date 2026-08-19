#!/usr/bin/env python3
"""R1726 §5.21 §5.35 §5.39 §2 #7 — **what a surface holds paints in front.**

Drag a node card over another one and it went UNDERNEATH. The owner reported it
as three complaints — *the card goes grey*, *they do not overlap*, *is not
overlapping better?* — and all three were this one fact:

* it does not go grey: the stationary card's opaque body is drawn over it, so
  only the dim row labels of the held card survive;
* they do overlap: after the drag the two sit two pixels apart;
* so the question is not whether to allow overlap — the reference allows it and
  so do we — it is that **the thing you picked up did not look picked up**.

Paint order IS z-order here: a scene draws depth-first and `Scene::hit_test`
walks the same children in reverse, so the last child is both drawn last and hit
first. That is why `ContainerNode::with_held` reorders rather than setting a
flag some later pass consults — a second source for that order is a second
source of truth, and this tree keeps meeting what happens then.

Measured before this round, on BOTH of the tree's node graphs:

    hello-node-lab     dragged index 70, stationary 80  -> BEHIND
    hello-node-editor  dragged index 12, stationary 15  -> BEHIND

Floor, measured by building a probe against 6.11.1 and running it: it has a
per-item z-value and its paint and hit orders agree — but **pressing or dragging
a movable item does not raise it** (the back item stayed behind through press,
drag and release), there is no notion of *held* (`isSelected` and
`isUnderMouse` exist; neither means "is being dragged"), and elevation does not
derive from it. A mechanism, and no rule — which is why applications there write
the raise themselves, and why two screens here got it wrong independently.

WHAT THIS SCRIPT DELIBERATELY DOES NOT DRIVE, and why that is a finding rather
than a gap in the script: the analysis tool's DASHBOARD is the third consumer of
the same capability, and this harness cannot perform its gesture. Measured on
one screen, one card, two paths — `scene/drag` makes the dragged card follow the
cursor, while a real pointer leaves it in place, moves a snap mark and reflows on
release, because a tile board commits on release. Driving the board from here
would therefore assert something nobody does. The board's half is gated by unit
tests over its paint order and by a real-pointer probe, and the missing
capability — drive a real pointer and read the scene while the button is down —
is registered as `debt-a-harness-drag-is-not-the-gesture-a-person-makes`.

Run from the workspace root:
    cargo build -p hello-node-lab -p hello-node-editor --release
    python3 tools/demos/r1726_what_you_hold_is_in_front.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def paint_order(rects: dict, tag: str) -> int:
    """Where `tag` sits in the depth-first paint walk — which is its depth."""
    keys = list(rects)
    return keys.index(tag) if tag in keys else -1


def overlaps(a, b) -> bool:
    return not (
        a[0] + a[2] <= b[0] or b[0] + b[2] <= a[0] or a[1] + a[3] <= b[1] or b[1] + b[3] <= a[1]
    )


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    # ── (A) the lab: one card held ────────────────────────────────────────
    banner("A — hello-node-lab: the card you are dragging is in front")
    with RpcSubprocess("hello-node-lab", boot_grace=1.5) as app:
        rects = abs_rects_of(app.snapshot(source="paint"))
        cards = {
            t: r for t, r in rects.items() if t.startswith("lab.node.") and t.count(".") == 2
        }
        names = sorted(t.rsplit(".", 1)[-1] for t in cards)
        src, dst = names[0], names[4]
        s, d = cards[f"lab.node.{src}"], cards[f"lab.node.{dst}"]
        ok(f"A: the screen paints {len(cards)} node cards", len(cards) >= 6)

        at_rest = paint_order(rects, f"lab.node.{src}")
        ok(f"A: {src} paints behind {dst} at rest", at_rest < paint_order(rects, f"lab.node.{dst}"))

        app.drag(
            from_at=(s[0] + s[2] // 2, s[1] + 12),
            to_at=(d[0] + d[2] // 2, d[1] + 12),
            phase="begin",
        )
        app.tick(16)
        during = abs_rects_of(app.snapshot(source="paint"))
        held_at = paint_order(during, f"lab.node.{src}")
        still_at = paint_order(during, f"lab.node.{dst}")
        assert_eq(
            held_at > still_at,
            True,
            f"A: DURING the drag the held card is IN FRONT ({held_at} > {still_at}). "
            "Before this round it was behind, and the stationary card's opaque "
            "body covered it -- which is what read as 'it goes grey'",
        )
        ok(
            "A: and the two really are overlapping, so the order matters",
            overlaps(during[f"lab.node.{src}"], during[f"lab.node.{dst}"]),
        )
        # ★ The half that makes the reorder sufficient rather than cosmetic: a
        # press finds what the eye sees on top, because the hit test reads the
        # same order in reverse.
        centre = during[f"lab.node.{src}"]
        reach = app.request(
            "scene/pointer_reach",
            {"at": {"x": centre[0] + centre[2] // 2, "y": centre[1] + 8}},
        )
        ok("A: the pointer reaches the surface the held card is on", reach is not None)

        app.drag(
            from_at=(d[0] + d[2] // 2, d[1] + 12),
            to_at=(d[0] + d[2] // 2, d[1] + 12),
            phase="end",
        )
        app.tick(16)
        after = abs_rects_of(app.snapshot(source="paint"))
        ok(
            "A: releasing puts the card down where it was dropped",
            f"lab.node.{src}" in after,
        )
        assert_eq(
            overlaps(after[f"lab.node.{src}"], after[f"lab.node.{dst}"]),
            True,
            "A: and the cards STAY overlapped -- overlap was never forbidden, "
            "the reference allows free placement and so do we",
        )
        # ★★★★★ THE HALF THAT SURVIVES THE RELEASE. Lifting only while the
        # gesture lasts is not the fix: measured before this round, a card held
        # at paint index 101 went straight back to 70 the moment it landed, so
        # the card just placed was the hidden one. Asserted AFTER the release,
        # because "do they overlap" and "which is on top" are different
        # questions and the first draft of this check only asked the first.
        dropped_at = paint_order(after, f"lab.node.{src}")
        under_at = paint_order(after, f"lab.node.{dst}")
        assert_eq(
            dropped_at > under_at,
            True,
            f"A: AFTER THE DROP the card stays in front ({dropped_at} > "
            f"{under_at}). Picking a card up raises it for good -- its POSITION "
            "is untouched, because on a free canvas a drop displaces nothing",
        )
        # And the raise is a reorder, not a move: nothing else went anywhere.
        for name in names:
            if name == src:
                continue
            assert_eq(
                after[f"lab.node.{name}"][:2],
                rects[f"lab.node.{name}"][:2],
                f"A: {name} did not move -- a drop displaces no neighbour, "
                "which is the free-canvas rule every node editor keeps and "
                "which this tree's tile dashboard deliberately does not",
            )
        # The screen is still whole: a reorder must not lose, duplicate or
        # rename a card.
        assert_eq(
            sorted(t for t in after if t.startswith("lab.node.") and t.count(".") == 2),
            sorted(t for t in rects if t.startswith("lab.node.") and t.count(".") == 2),
            "A: the same cards are on the canvas after the drop -- raising is a "
            "permutation, not an edit",
        )
        for pane in ("lab.palette", "lab.canvas", "lab.inspector", "lab.appbar"):
            ok(f"A: the screen still paints {pane}", pane in after)
        # Picking a SECOND card up puts that one in front and leaves the first
        # ahead of everything it was already ahead of: the order is a history.
        second = next(n for n in names if n not in (src, dst))
        s2 = after[f"lab.node.{second}"]
        app.drag(
            from_at=(s2[0] + s2[2] // 2, s2[1] + 12),
            to_at=(s2[0] + s2[2] // 2 + 30, s2[1] + 40),
            phase="full",
        )
        app.tick(16)
        third = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(
            paint_order(third, f"lab.node.{second}") > paint_order(third, f"lab.node.{src}"),
            True,
            "A: the most recently picked-up card is the frontmost",
        )
        assert_eq(
            paint_order(third, f"lab.node.{src}") > paint_order(third, f"lab.node.{dst}"),
            True,
            "A: and the one picked up before it is still ahead of the one "
            "nobody has touched -- the stacking order is a history of what has "
            "been handled, not a single 'topmost' slot",
        )
        for name in names:
            if name in (src, second):
                continue
            ok(
                f"A: {name} is still on the canvas after two drags",
                f"lab.node.{name}" in third,
            )

    # ── (B) the editor: a whole selection held ────────────────────────────
    banner("B — hello-node-editor: a GROUP picked up stays one thing")
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as app:
        rects = abs_rects_of(app.snapshot(source="paint"))
        nodes = {t: r for t, r in rects.items() if "#node_" in t}
        ok(f"B: the screen paints {len(nodes)} nodes", len(nodes) >= 2)
        keys = list(nodes)
        src, dst = keys[0], keys[1]
        s, d = nodes[src], nodes[dst]
        ok("B: the dragged node starts behind", paint_order(rects, src) < paint_order(rects, dst))

        app.drag(
            from_at=(s[0] + s[2] // 2, s[1] + 8),
            to_at=(d[0] + d[2] // 2, d[1] + 8),
            phase="begin",
        )
        app.tick(16)
        during = abs_rects_of(app.snapshot(source="paint"))
        held_at, still_at = paint_order(during, src), paint_order(during, dst)
        assert_eq(
            held_at > still_at,
            True,
            f"B: the held node is IN FRONT ({held_at} > {still_at}) on the "
            "SECOND consumer too -- which is what makes this a framework "
            "capability rather than one screen's repair",
        )
        app.drag(
            from_at=(d[0] + d[2] // 2, d[1] + 8),
            to_at=(d[0] + d[2] // 2, d[1] + 8),
            phase="end",
        )
        app.tick(16)
        after = abs_rects_of(app.snapshot(source="paint"))
        ok("B: and the drag settles", "#node_" in " ".join(after))
        # The persistent half on this screen too: dropped, it stays in front.
        assert_eq(
            paint_order(after, src) > paint_order(after, dst),
            True,
            "B: the node stays in front after the drop here as well -- the lift "
            "is not tied to the gesture still being held",
        )
        assert_eq(
            sorted(t for t in after if "#node_" in t),
            sorted(t for t in rects if "#node_" in t),
            "B: and the same nodes are on the canvas -- a reorder, not an edit",
        )
        for tag in keys:
            ok(f"B: {tag.rsplit('#', 1)[-1]} survived the drag", tag in after)
        # ★ The panic this demo caught on its first run: the press path reached
        # the owner cache, which it has no scope for. A second press is the
        # cheapest possible guard against that returning.
        n2 = after[keys[-1]]
        app.drag(
            from_at=(n2[0] + n2[2] // 2, n2[1] + 8),
            to_at=(n2[0] + n2[2] // 2 + 40, n2[1] + 40),
            phase="full",
        )
        app.tick(16)
        ok(
            "B: a second press does not take the screen down",
            keys[-1] in abs_rects_of(app.snapshot(source="paint")),
        )

    print(f"\n[demo] {len(CHECKS)} named check(s)")
    ok("what a surface holds paints in front of what it does not", True)


run_demo("R1726 what you hold is in front", body)
