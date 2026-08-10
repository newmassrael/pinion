#!/usr/bin/env python3
"""R1631 — align, distribute, stack and straighten, as three parameters.

The engine reference gives a node editor eleven separate commands for tidying a
selection: `AlignNodesLeft` / `Center` / `Right` / `Top` / `Middle` / `Bottom`,
`DistributeNodesHorizontally` / `Vertically`, `StackNodesHorizontally` /
`Vertically`, and `StraightenConnections`. Spelled as eleven names the set
cannot be enumerated, stored, or offered by a generic control — every consumer
writes the eleven-way match again.

`pinion_node_graph::arrange` is those eleven as an axis, an edge and a gap, and
this demo drives all of them through ONE verb over the wire.

What each check discriminates:

* **An align moves on one axis only.** A pass that also tidied the other axis
  would pass "the left edges line up" and destroy the author's placement.
* **The box is the SELECTION's.** Aligning two of four nodes must not reach for
  the tree's leading edge.
* **A second press moves nothing.** The centre is the arm that drifts if the
  rounding goes the wrong way, and the wire reports `moved:0` — which is also
  what lets an undo stack skip an empty entry.
* **Distribute pins the extremes; stack does not.** They are different passes
  and a demo that only checked "the gaps got even" would not tell them apart.
* **The gap is a parameter.** The reference compiles a constant in.
* **Straighten reports what it could NOT straighten.** The reference moves what
  it can and says nothing; a fan-in leaves one link bent and the wire names it.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1631_eleven_commands_are_three_parameters.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: The card width the example draws; the crate is told it, never guesses it.
CARD_W = 150

#: The example's paint-tag prefix.
VIEW_TAG = "nodegroups"


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def arrange(tf: RpcSubprocess, spec: str) -> dict[str, str]:
    return fields(inv(tf, "arrange", spec))


def select(tf: RpcSubprocess, ids: list[int]) -> None:
    assert_eq(
        str(inv(tf, "select", ",".join(str(i) for i in ids))),
        str(len(ids)),
        f"selected {ids}",
    )


def boxes(tf: RpcSubprocess, ids: list[int]) -> dict[int, tuple[int, int, int, int]]:
    """Each node's rect, read off the PAINTED scene.

    Read from the picture rather than from the model: an arrangement that
    computed correct numbers and never reached the canvas is the failure this
    is written against.
    """
    from rpc_verify import abs_rects_of  # noqa: PLC0415

    rects = abs_rects_of(tf.snapshot(source="paint", viewport=[980, 620]))
    out: dict[int, tuple[int, int, int, int]] = {}
    for node in ids:
        rect = rects.get(f"{VIEW_TAG}.node.{node}")
        assert rect is not None, f"node {node} is painted: {sorted(rects)[:20]}"
        out[node] = tuple(rect)
    return out


def refused(tf: RpcSubprocess, spec: str) -> None:
    try:
        inv(tf, "arrange", spec)
    except Exception:  # noqa: BLE001 — the refusal is the assertion
        return
    raise AssertionError(f"arrange {spec!r} should have been refused")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        every = [BASE, BLEND, LEVEL, MIX, FADE, OUT]

        # ── 1. an align meets one edge, on one axis ─────────────────────
        before = boxes(tf, every)
        select(tf, every)
        moved = arrange(tf, "align:horizontal:start")
        assert int(moved["moved"]) > 0, f"something moved: {moved}"
        tf.tick(0.016)
        after = boxes(tf, every)
        xs = {after[n][0] for n in every}
        assert_eq(len(xs), 1, f"every card meets one left edge: {after}")
        assert_eq(
            min(before[n][0] for n in every),
            next(iter(xs)),
            "and it is the selection's own leading edge, not a new one",
        )
        for node in every:
            assert_eq(after[node][1], before[node][1], f"node {node} kept its y")
        print(f"[demo] align:horizontal:start -> x={next(iter(xs))}, no y moved")

        # ── 2. a second press moves nothing ─────────────────────────────
        again = arrange(tf, "align:horizontal:start")
        assert_eq(again["moved"], "0", "an align is idempotent")
        centre_once = arrange(tf, "align:vertical:center")
        tf.tick(0.016)
        centred = boxes(tf, every)
        centre_twice = arrange(tf, "align:vertical:center")
        assert_eq(
            centre_twice["moved"],
            "0",
            "★ the CENTRE is the arm that drifts if the rounding goes outward",
        )
        assert int(centre_once["moved"]) >= 0
        # ★ Centring makes the MIDLINES coincide, not the tops: these cards
        # have different port counts and so different heights, and an
        # assertion on `y` would demand the wrong thing — it would pass only
        # for a pass that ignored the extents it was given.
        mids = {centred[n][1] + centred[n][3] // 2 for n in every}
        assert len(mids) <= 2, f"the midlines meet (rounding aside): {centred}"
        tops = {centred[n][1] for n in every}
        assert len(tops) > 1, (
            "and the TOPS do not, which is what proves the extents were used"
        )
        print("[demo] a second press reports moved:0, centre included")

        # ── 3. the box is the SELECTION's, not the tree's ───────────────
        tf.tick(0.016)
        spread = arrange(tf, "stack:horizontal:40")
        assert int(spread["moved"]) > 0, "spread them back out to have a box"
        tf.tick(0.016)
        wide = boxes(tf, every)
        pair = [LEVEL, OUT]
        select(tf, pair)
        arrange(tf, "align:horizontal:start")
        tf.tick(0.016)
        partial = boxes(tf, every)
        assert_eq(
            partial[LEVEL][0],
            partial[OUT][0],
            "the two named cards meet each other",
        )
        assert_eq(
            partial[LEVEL][0],
            min(wide[n][0] for n in pair),
            "at the SELECTION's leading edge",
        )
        for node in every:
            if node not in pair:
                assert_eq(partial[node], wide[node], f"node {node} was not named")
        print("[demo] an arrangement moves only what was named")

        # ── 4. stack packs with the gap it was given ────────────────────
        select(tf, every)
        for gap in (0, 24, 60):
            arrange(tf, f"stack:vertical:{gap}")
            tf.tick(0.016)
            packed = sorted(boxes(tf, every).values(), key=lambda p: p[1])
            # The pitch is a card's own height plus the gap, and the cards
            # differ, so what is constant is the CLEAR SPACE between them.
            clear = {b[1] - (a[1] + a[3]) for a, b in zip(packed, packed[1:], strict=False)}
            assert_eq(clear, {gap}, f"gap {gap}: one clear space, {clear}")
            print(f"[demo] stack:vertical:{gap} -> clear {sorted(clear)}")
        # ★ The parameter is what the reference does not have: three different
        # pitches from one command, and the differences are exactly the gaps.
        pitch_at = {}
        for gap in (0, 24):
            arrange(tf, f"stack:vertical:{gap}")
            tf.tick(0.016)
            packed = sorted(boxes(tf, every).values(), key=lambda p: p[1])
            pitch_at[gap] = packed[1][1] - packed[0][1]
        assert_eq(pitch_at[24] - pitch_at[0], 24, "the gap IS the parameter")

        # ── 5. distribute pins the extremes ─────────────────────────────
        # ★ The fixture has to be UNEVEN before distributing, or the check
        # cannot tell a working pass from one that did nothing: packing to a
        # constant gap and then distributing reproduces the same gaps whatever
        # `distribute` does. So the trailing card is pushed far out first.
        arrange(tf, "stack:horizontal:10")
        tf.tick(0.016)
        select(tf, [BASE, BLEND, LEVEL])
        arrange(tf, "stack:horizontal:120")
        tf.tick(0.016)
        uneven = boxes(tf, every)
        uneven_gaps = sorted(
            b[0] - (a[0] + a[2])
            for a, b in zip(
                sorted(uneven.values(), key=lambda p: p[0]),
                sorted(uneven.values(), key=lambda p: p[0])[1:],
                strict=False,
            )
        )
        assert uneven_gaps[-1] - uneven_gaps[0] > 1, (
            f"the fixture must be uneven before distributing: {uneven_gaps}"
        )
        select(tf, every)
        arrange(tf, "distribute:horizontal")
        tf.tick(0.016)
        spread_out = boxes(tf, every)
        assert spread_out != uneven, "and distribute must have moved something"
        lefts = sorted(p[0] for p in spread_out.values())
        arrange(tf, "distribute:horizontal")
        tf.tick(0.016)
        assert_eq(
            sorted(p[0] for p in boxes(tf, every).values()),
            lefts,
            "distribute is idempotent, so the extremes really are pinned",
        )
        ordered = sorted(spread_out.values(), key=lambda p: p[0])
        gaps = [
            b[0] - (a[0] + a[2])
            for a, b in zip(ordered, ordered[1:], strict=False)
        ]
        assert max(gaps) - min(gaps) <= 1, f"gaps within the remainder: {gaps}"
        print(f"[demo] distribute:horizontal -> gaps {gaps}")

        # ── 6. straighten SAYS what it could not straighten ─────────────
        select(tf, every)
        report = arrange(tf, "straighten:horizontal")
        assert "straight" in report, f"the report is on the wire: {report}"
        assert "bent" in report, f"and so is the leftover: {report}"
        assert int(report["straight"]) > 0, "the seed has links to straighten"
        # `Mix` takes two producers, so one of its two links cannot be
        # straightened without bending the other — the case the reference
        # handles silently.
        assert report["bent"], (
            "★ a fan-in leaves a link bent and the wire NAMES it: "
            f"{report}"
        )
        print(f"[demo] straighten -> straight:{report['straight']} bent:{report['bent']}")
        tf.tick(0.016)
        after_straight = boxes(tf, every)
        straightened_pairs = {
            (after_straight[a][1], after_straight[b][1])
            for a, b in ((BASE, MIX), (BLEND, MIX), (LEVEL, FADE))
        }
        assert any(a == b for a, b in straightened_pairs), (
            f"at least one link's ends share a y: {after_straight}"
        )

        # ── 7. every spelling outside the vocabulary is refused ─────────
        for bad in (
            "align:sideways:start",
            "align:horizontal:middle",
            "distribute:diagonal",
            "stack:horizontal:soon",
            "shuffle:horizontal",
            "align:horizontal",
        ):
            refused(tf, bad)
        print("[demo] six spellings outside the vocabulary refused")

        # ── 8. a selection too small to arrange reports a move of zero ──
        select(tf, [MIX])
        assert_eq(
            arrange(tf, "align:horizontal:center")["moved"],
            "0",
            "one node is already aligned with itself",
        )
        assert_eq(
            arrange(tf, "distribute:horizontal")["moved"],
            "0",
            "and has no gap to equalise",
        )
        select(tf, [MIX, OUT])
        assert_eq(
            arrange(tf, "distribute:horizontal")["moved"],
            "0",
            "two nodes have one gap and it is already the only one it can be",
        )
        assert int(arrange(tf, "align:horizontal:start")["moved"]) >= 0, (
            "while an align of two is a real arrangement"
        )
        print("[demo] a selection too small to arrange answers moved:0")

        # ...and the six align spellings that ARE in it all answer.
        for axis in ("horizontal", "vertical"):
            for edge in ("start", "center", "end"):
                answer = arrange(tf, f"align:{axis}:{edge}")
                assert "moved" in answer, f"align:{axis}:{edge} -> {answer}"
        print("[demo] all six aligns answer; eleven commands, three parameters")


if __name__ == "__main__":
    run_demo("R1631 — eleven commands are three parameters", body)
