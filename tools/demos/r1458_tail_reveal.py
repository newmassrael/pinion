#!/usr/bin/env python3
"""R1458 §5.45 §5.27 settle-before-paint — `hello-tail-reveal`.

`hello-transcript` (R1445) already arms `follow_measured_tail` on a transcript
of wrapped prose. Its transcript is a plain column, so every pass lays out
every entry and the first bound the frame publishes is already the true one.

Virtualize the same transcript and it stops being true. A windowed list only
lays out the rows it materialized, so the harvest can only measure THOSE; the
rest — including every row near the tail — are still counted at the estimate.
The bound the first pass publishes is provisional, and it is exactly the pass a
one-shot pin used to be spent on: the viewport landed on the estimate-derived
tail, the next pass measured the rows that arrival brought into the window, the
bound grew past where the reader now sat, and nothing was left armed to carry
them the rest of the way.

Proven as DATA through `scene/snapshot` + `scene/scroll_state` +
`scene/query` (§2 #7, no pixels):

  - boot: 60 entries, most of them never laid out — `is_fully_measured` is
    False and `measured_count` is a fraction of `item_count`. THAT is the
    precondition; without it this demo would pass on the broken build too.
  - Reply: the newest entry is on screen **in the frame the reply was
    asked for**, with its bottom edge resting on the viewport's, and
    `offset == max == total_height - viewport`. Every one of those numbers
    comes from a different surface than the others.
  - the newest entry's measured height is far past the estimate the
    provisional bound counted it at — the exact distance a pre-R1458 pin fell
    short by.
  - one-shot: after the pin is spent, scrolling back STAYS back across further
    frames. A pin that had become a mode would yank the reader every frame.

What this demo does NOT cover: R1458's other half — that the paint runs to the
fixed point *before presenting*, and asks for another frame when it cannot.
Every RPC call here incidentally runs a layout pass, so the state converges
between requests no matter how few passes one paint spends; capping the
producer's budget at the pre-R1458 two leaves this demo green. That half is
pinned by `crates/pinion-shell/tests/frame_settles_before_paint.rs`, which can
observe a single paint's own passes.

ZERO-FLAKE: every step is a deterministic `scene/click` / `scene/scroll`, every
assertion reads published data, and no expectation is a font constant — the
heights are read back from the same run that produced them.

Run from the workspace root:
    cargo build -p hello-tail-reveal --release
    python3 tools/demos/r1458_tail_reveal.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-tail-reveal"
WIN = (520, 620)

# Mirrors of the binding's declared geometry — note that NONE of them is a
# height.
VIEWPORT_H = 300
EST = 24
SEED = 60

SCROLL_TAG = "reveal_scroll"
STATUS_TAG = "reveal_status"
REPLY_TAG = "reveal_reply"
ROW_SEG = "measured-row:"

DOT = "·"


# ── published-state readers ────────────────────────────────────────────────


def scroll_state(tf) -> dict:
    """`scene/scroll_state` — offset, bound, edges, and the standing arming."""
    return tf.request("scene/scroll_state", {"tag": SCROLL_TAG}).result


def table(tf, field: str):
    """The measurement table, through the primary External's query channel."""
    return tf.query(f"/external/{field}")


def paint(tf):
    return tf.snapshot(source="paint", viewport=WIN)


def windowed_slots(snap) -> dict[int, tuple[int, int, int, int]]:
    """Row index -> window-absolute rect, for the rows this frame MATERIALIZED.

    The slot tag is scroll-scoped (`<scroll>/measured-row:<i>`, R1199), so the
    index is read off the last segment.
    """
    out = {}
    for tag, rect in abs_rects_of(snap).items():
        if ROW_SEG in tag:
            out[int(tag.rsplit(ROW_SEG, 1)[1])] = rect
    return out


def status_of(snap):
    node = find_by_tag(snap, STATUS_TAG)
    if node is None:
        return None
    for child in node.get("children", []) or []:
        if child.get("type") == "Text":
            return child.get("content")
    return None


def status_line(count: int, following: bool) -> str:
    return f"{count} entries {DOT} {'at the tail' if following else 'scrolled back'}"


# ── the two claims ─────────────────────────────────────────────────────────


def assert_bound_is_measured(tf, label: str) -> int:
    """The bound equals the measurement table's own total, minus the viewport.

    Two independently-produced numbers: `max` is written by the layout pass
    from the laid-out sizer, `total_height` is the harvest's table. A binding
    that inflated the bound to dodge the clamp (the pre-R1445 idiom) fails this
    by ~2 billion.
    """
    state = scroll_state(tf)
    max_y = int(state["max"]["y"])
    assert_eq(
        max_y,
        max(0, int(table(tf, "total_height")) - VIEWPORT_H),
        f"{label}: bound == measured extent - viewport",
    )
    assert max_y < 1_000_000, f"{label}: bound {max_y} is not a real extent"
    return max_y


def assert_revealed(tf, label: str, count: int):
    """The newest entry is ON SCREEN, in the frame the reply was asked for."""
    snap = paint(tf)
    newest = count - 1
    state = scroll_state(tf)

    assert_eq(
        state["following_measured_tail"],
        False,
        f"{label}: the arming was spent, not left standing",
    )
    assert_eq(
        int(state["offset"]["y"]),
        int(state["max"]["y"]),
        f"{label}: the viewport sits at the bound",
    )
    assert_eq(state["edges"]["at_bottom"], True, f"{label}: at_bottom edge")
    assert_bound_is_measured(tf, label)
    assert_eq(status_of(snap), status_line(count, True), f"{label}: status")

    slots = windowed_slots(snap)
    assert newest in slots, (
        f"{label}: the newest entry #{newest} is not even materialized "
        f"(window = {sorted(slots)})"
    )
    scroll_x, scroll_y, _, scroll_h = abs_rects_of(snap)[SCROLL_TAG]
    row_x, row_y, _, row_h = slots[newest]
    assert_eq(row_x, scroll_x, f"{label}: newest entry is inside the viewport")
    assert row_y >= scroll_y, (
        f"{label}: newest entry top {row_y} is below the viewport top {scroll_y}"
    )
    assert_eq(
        row_y + row_h,
        scroll_y + scroll_h,
        f"{label}: newest entry's bottom rests on the viewport's — 'pinned' "
        f"means visible, not merely offset == max",
    )

    # The precondition, read from the same run: this row is far taller than the
    # estimate a provisional bound counted it at. The gap IS the distance a
    # pre-R1458 pin fell short by.
    measured = table(tf, f"measured_height.{newest}")
    assert measured is not None, f"{label}: newest entry was never measured"
    assert_eq(int(measured), row_h, f"{label}: table height == laid-out height")
    assert int(measured) > EST, (
        f"{label}: newest entry measured {measured}px against a {EST}px "
        f"estimate — if these were equal the test would prove nothing"
    )
    return snap


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: a backlog most of which has never been laid out ───────────
        snap = paint(tf)
        assert_eq(int(table(tf, "item_count")), SEED, "boot: entry count")
        assert_eq(int(table(tf, "estimated")), EST, "boot: the estimate")
        assert_eq(int(table(tf, "viewport_h")), VIEWPORT_H, "boot: viewport")
        state = scroll_state(tf)
        assert_eq(int(state["offset"]["y"]), 0, "boot: the reader is at the top")
        assert_eq(state["edges"]["at_top"], True, "boot: at_top edge")
        assert_eq(
            state["following_measured_tail"], False, "boot: nothing armed"
        )
        boot_max = assert_bound_is_measured(tf, "boot")
        assert boot_max > 0, "boot: the backlog overflows the viewport"
        assert_eq(
            status_of(snap), status_line(SEED, False), "boot: status"
        )

        # THE PRECONDITION. Most rows are still counted at the estimate, so the
        # bound above is provisional — which is what makes the first pass the
        # wrong pass to spend a pin on.
        boot_measured = int(table(tf, "measured_count"))
        assert_eq(
            table(tf, "is_fully_measured"),
            False,
            "boot: the tail has never been laid out",
        )
        assert boot_measured < SEED, (
            f"boot: {boot_measured} of {SEED} rows measured — a fully-measured "
            f"table would make every bound exact and prove nothing"
        )
        slots = windowed_slots(snap)
        assert_eq(
            sorted(slots),
            list(range(min(slots), max(slots) + 1)),
            "boot: the window is one contiguous run",
        )
        assert SEED - 1 not in slots, "boot: the newest entry is below the fold"

        # ── reply: revealed, in the frame it was asked for ──────────────────
        count = SEED
        for round_no in (1, 2, 3):
            tf.click(path=REPLY_TAG)
            count += 1
            assert_eq(
                int(table(tf, "item_count")), count, f"reply {round_no}: appended"
            )
            assert_revealed(tf, f"reply {round_no}", count)

        # Each reply grew the bound: the reveal is tracking new content, not
        # sitting on a bound that stopped moving.
        grown_max = int(scroll_state(tf)["max"]["y"])
        assert grown_max > boot_max, "the bound grew with the appended entries"

        # ── one-shot: scrolling back STAYS back, across frames ──────────────
        tf.scroll(SCROLL_TAG, to=(0, 0))
        assert_eq(int(scroll_state(tf)["offset"]["y"]), 0, "scrolled back to 0")
        assert_eq(
            scroll_state(tf)["following_measured_tail"],
            False,
            "no arming left standing to yank the reader back",
        )
        for _ in range(4):
            tf.tick(0.05)
            assert_eq(
                int(scroll_state(tf)["offset"]["y"]),
                0,
                "a spent pin does not re-fire on later frames",
            )
        back = paint(tf)
        assert_eq(
            status_of(back), status_line(count, False), "scrolled back: status"
        )
        assert count - 1 not in windowed_slots(back), (
            "scrolled back: the newest entry is off screen again"
        )

        # ── and a reply from up there reveals it again ──────────────────────
        tf.click(path=REPLY_TAG)
        count += 1
        assert_revealed(tf, "reply from the top", count)


if __name__ == "__main__":
    run_demo("R1458 layout-settled tail reveal (hello-tail-reveal)", body)
