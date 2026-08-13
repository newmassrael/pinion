#!/usr/bin/env python3
"""R1445 §5.45 §5.27 layout-measured tail pin — `hello-transcript`.

A transcript of **wrapped prose**. Its height is whatever parley's line breaker
decided, so — unlike `hello-streaming-log`'s uniform rows — the binding cannot
compute the post-append bound and hand it to `follow_tail`. It computes nothing:
it arms `ScrollState::follow_measured_tail`, and the layout pass pins the
viewport to the extent it measured.

Proven as DATA through `scene/snapshot` + `scene/scroll_state` (§2 #7, no
pixels):

  - boot: a backlog taller than the viewport; the reader is at the top, the
    newest entry is off the bottom.
  - Notice (ambient) while scrolled back: the entry appends, the bound grows,
    and the viewport does NOT move — `tail -f` etiquette.
  - Reply (the answer to what the reader pressed) while scrolled back: the
    viewport lands on the tail, bottom-aligned to the padding.
  - the bound is never faked. Every step recomputes it INDEPENDENTLY from the
    published entry geometry (`last entry bottom + padding - viewport`) and
    requires the published bound to equal it — the check the pre-R1445
    `set_max(0, i32::MAX)` idiom fails by 2 billion.
  - one-shot: after a pin, scrolling back stays scrolled back, across further
    frames and further ambient appends. A standing (not consumed) arming would
    yank the reader back every frame.

ZERO-FLAKE: every step is a deterministic `scene/click` / `scene/scroll`, and
every wait is a `wait_snap` predicate on published data — no wall-clock sleeps.

Run from the workspace root:
    cargo build -p hello-transcript --release
    python3 tools/demos/r1445_transcript.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    unclipped_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-transcript"
WIN = (480, 620)

# Mirrors of the binding's geometry constants (the only numbers it declares —
# note that NONE of them is a height).
VIEWPORT_H = 300
PAD = 12
GAP = 10

SCROLL_TAG = "transcript_scroll"
ENTRY_TAG = "entry"
STATUS_TAG = "transcript_status"
REPLY_TAG = "transcript_reply"
NOTICE_TAG = "transcript_notice"

DOT = "·"

# Mirrors of the binding's text rule: bodies rotate per kind, on that kind's
# own running index, and the seed backlog is built through the same rule.
REPLIES = [
    "The pass runs after every bound writer, so the offset it pins is the one "
    "the frame ended with.",
    "A bound is measured, never asserted. The binding says what it wants; the "
    "layout pass says how far that is. Nothing in between has to guess, and no "
    "reader of this state ever sees a number that was true for only one frame.",
    "Reply and notice differ in policy, not in machinery.",
    "Wrapped prose has no pitch to multiply. Its height is whatever the line "
    "breaker decided, which is why the arming carries no number at all — the "
    "one thing the caller genuinely cannot supply.",
]
NOTICES = [
    "A background task finished.",
    "Two peers reconnected; the queue drained without a retry.",
    "Nothing needed attention for the last interval.",
]
SEED = ["notice", "reply", "reply", "notice", "reply", "reply", "notice"]


def entry_text(kind: str, nth: int) -> str:
    pool = REPLIES if kind == "reply" else NOTICES
    label = "Reply" if kind == "reply" else "Notice"
    return f"{label} {nth}: {pool[nth % len(pool)]}"


def expected_texts(history: list[str]) -> list[str]:
    """The full transcript implied by an append history (seed included)."""
    seen = {"reply": 0, "notice": 0}
    out = []
    for kind in history:
        out.append(entry_text(kind, seen[kind]))
        seen[kind] += 1
    return out


# ── published-state readers ────────────────────────────────────────────────


def scroll_state(tf) -> dict:
    """`scene/scroll_state` — offset, bound, edges, and the standing-arming bit."""
    return tf.request("scene/scroll_state", {"tag": SCROLL_TAG}).result


def entry_count(snap) -> int:
    n = 0
    while find_by_tag(snap, f"{ENTRY_TAG}#{n}") is not None:
        n += 1
    return n


def entry_texts(snap) -> list[str]:
    out = []
    n = 0
    while (node := find_by_tag(snap, f"{ENTRY_TAG}#{n}")) is not None:
        out.append(node.get("content"))
        n += 1
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


def last_entry_local_bottom(snap) -> int:
    """Bottom edge of the newest entry in SCROLL-LOCAL coordinates."""
    n = entry_count(snap) - 1
    rect = find_by_tag(snap, f"{ENTRY_TAG}#{n}")["rect"]
    return int(rect["y"]) + int(rect["h"])


def measured_bound(snap) -> int:
    """Recompute the scroll bound from the published entry geometry alone.

    `content extent - viewport`, where the extent is the newest entry's bottom
    plus the content's bottom padding. This never consults the reported bound,
    so comparing the two is a real check: a binding that inflated `max_y` to
    dodge the clamp (the pre-R1445 idiom) fails it by ~2 billion.
    """
    return max(0, last_entry_local_bottom(snap) + PAD - VIEWPORT_H)


def assert_bound_is_measured(tf, snap, label: str) -> int:
    state = scroll_state(tf)
    max_y = int(state["max"]["y"])
    assert_eq(max_y, measured_bound(snap), f"{label}: bound == measured extent")
    assert max_y < 1_000_000, f"{label}: bound {max_y} is not a real extent"
    assert_eq(
        state["following_measured_tail"],
        False,
        f"{label}: no arming left standing (the pass consumed it)",
    )
    return max_y


def assert_pinned_to_tail(tf, snap, label: str) -> None:
    """The viewport sits at the measured tail, and the newest entry is ON SCREEN
    with its bottom edge one padding above the viewport's — the geometric
    witness that 'pinned' means visible, not merely `offset == max`."""
    state = scroll_state(tf)
    assert_eq(
        int(state["offset"]["y"]),
        int(state["max"]["y"]),
        f"{label}: offset sits at the bound",
    )
    assert_eq(state["edges"]["at_bottom"], True, f"{label}: at_bottom edge")
    rects = abs_rects_of(snap)
    scroll_x, scroll_y, _, scroll_h = rects[SCROLL_TAG]
    last = f"{ENTRY_TAG}#{entry_count(snap) - 1}"
    _, entry_y, _, entry_h = rects[last]
    assert entry_y >= scroll_y, f"{label}: {last} top is inside the viewport"
    assert_eq(
        entry_y + entry_h,
        scroll_y + scroll_h - PAD,
        f"{label}: {last} bottom rests on the content padding",
    )


def assert_below_the_fold(snap, label: str) -> None:
    # ★ R1676 — the PLACEMENT reader, and this function is why the two readers
    # are two: its whole claim is that the newest entry is painted where the
    # viewport does not show it. `abs_rects_of` answers what is on screen and
    # therefore does not carry a mark that is entirely off it — asking it here
    # is asking for the rectangle of the thing whose absence is the point.
    # `assert_pinned_to_tail` above keeps the visible reader for the mirror
    # reason: "pinned" is a claim that the entry CAN be seen.
    rects = unclipped_rects_of(snap)
    _, scroll_y, _, scroll_h = rects[SCROLL_TAG]
    last = f"{ENTRY_TAG}#{entry_count(snap) - 1}"
    entry_y = rects[last][1]
    assert entry_y >= scroll_y + scroll_h, (
        f"{label}: {last} is off the bottom (top {entry_y} vs viewport bottom "
        f"{scroll_y + scroll_h})"
    )


def snap_with_entries(tf, n: int, desc: str):
    return wait_snap(
        tf, lambda s: entry_count(s) == n, viewport=WIN, desc=desc
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        history = list(SEED)

        # ── boot: a backlog taller than the viewport, read from the top ─────
        snap = snap_with_entries(tf, len(SEED), "boot: seeded backlog")
        assert_eq(entry_texts(snap), expected_texts(history), "boot: entry text")
        state = scroll_state(tf)
        assert_eq(int(state["offset"]["y"]), 0, "boot: reader is at the top")
        boot_max = assert_bound_is_measured(tf, snap, "boot")
        assert boot_max > 0, "boot: the backlog overflows the viewport"
        assert_eq(state["edges"]["at_top"], True, "boot: at_top edge")
        assert_eq(
            status_of(snap), status_line(len(SEED), False), "boot: status"
        )
        assert_below_the_fold(snap, "boot")

        # ── ambient append while scrolled back → the viewport must NOT move ──
        tf.click(path=NOTICE_TAG)
        history.append("notice")
        snap = snap_with_entries(tf, len(history), "notice while scrolled back")
        assert_eq(entry_texts(snap), expected_texts(history), "notice: entry text")
        paused_max = assert_bound_is_measured(tf, snap, "notice")
        assert paused_max > boot_max, "notice: the bound grew with the content"
        assert_eq(
            int(scroll_state(tf)["offset"]["y"]),
            0,
            "notice: ambient traffic does not yank a reader who scrolled back",
        )
        assert_eq(
            status_of(snap), status_line(len(history), False), "notice: status"
        )
        assert_below_the_fold(snap, "notice")

        # ── reply while scrolled back → revealed, pinned to the MEASURED tail ─
        tf.click(path=REPLY_TAG)
        history.append("reply")
        snap = wait_snap(
            tf,
            lambda s: entry_count(s) == len(history)
            and status_of(s) == status_line(len(history), True),
            viewport=WIN,
            desc="reply while scrolled back: revealed",
        )
        assert_eq(entry_texts(snap), expected_texts(history), "reply: entry text")
        reply_max = assert_bound_is_measured(tf, snap, "reply")
        assert reply_max > paused_max, "reply: the bound grew again"
        assert_pinned_to_tail(tf, snap, "reply")

        # ── a second reply: the bound grows by exactly the new entry ─────────
        prev_bottom = last_entry_local_bottom(snap)
        tf.click(path=REPLY_TAG)
        history.append("reply")
        snap = snap_with_entries(tf, len(history), "reply 2: still at the tail")
        new_rect = find_by_tag(snap, f"{ENTRY_TAG}#{len(history) - 1}")["rect"]
        assert_eq(
            last_entry_local_bottom(snap),
            prev_bottom + GAP + int(new_rect["h"]),
            "reply 2: extent grew by gap + the measured height of the new entry",
        )
        second_max = assert_bound_is_measured(tf, snap, "reply 2")
        assert_eq(
            second_max,
            reply_max + GAP + int(new_rect["h"]),
            "reply 2: the bound tracked that growth exactly",
        )
        assert_pinned_to_tail(tf, snap, "reply 2")

        # ── one-shot: scrolling back STAYS back, across frames ──────────────
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf,
            lambda s: status_of(s) == status_line(len(history), False),
            viewport=WIN,
            desc="scroll back to the top",
        )
        assert_eq(int(scroll_state(tf)["offset"]["y"]), 0, "scrolled back to 0")
        assert_eq(
            scroll_state(tf)["following_measured_tail"],
            False,
            "the reply's arming was consumed, not left standing",
        )
        for _ in range(3):
            tf.tick(0.05)
        snap = wait_snap(
            tf,
            lambda s: entry_count(s) == len(history),
            viewport=WIN,
            desc="frames pass with no re-pin",
        )
        assert_eq(
            int(scroll_state(tf)["offset"]["y"]),
            0,
            "one-shot: later frames do not re-pin (a mode would yank us back)",
        )
        assert_eq(int(scroll_state(tf)["max"]["y"]), second_max, "bound unchanged")

        # ── ambient append from the top: still paused ────────────────────────
        tf.click(path=NOTICE_TAG)
        history.append("notice")
        snap = snap_with_entries(tf, len(history), "notice from the top: paused")
        assert_eq(
            int(scroll_state(tf)["offset"]["y"]),
            0,
            "notice from the top leaves the reader where they were",
        )
        assert_bound_is_measured(tf, snap, "notice from the top")
        assert_below_the_fold(snap, "notice from the top")

        # ── a reply always reveals, even from the very top ───────────────────
        tf.click(path=REPLY_TAG)
        history.append("reply")
        snap = wait_snap(
            tf,
            lambda s: entry_count(s) == len(history)
            and status_of(s) == status_line(len(history), True),
            viewport=WIN,
            desc="reply from the top: revealed",
        )
        assert_bound_is_measured(tf, snap, "reply from the top")
        assert_pinned_to_tail(tf, snap, "reply from the top")
        assert_eq(entry_texts(snap), expected_texts(history), "reply: entry text")

        # ── ambient append while AT the tail → follow resumes ────────────────
        tf.click(path=NOTICE_TAG)
        history.append("notice")
        snap = wait_snap(
            tf,
            lambda s: entry_count(s) == len(history)
            and status_of(s) == status_line(len(history), True),
            viewport=WIN,
            desc="notice while at the tail: follows",
        )
        assert_bound_is_measured(tf, snap, "notice at the tail")
        assert_pinned_to_tail(tf, snap, "notice at the tail")
        assert_eq(
            entry_texts(snap)[-1],
            expected_texts(history)[-1],
            "the followed entry is the one that appended",
        )

        # ── keyboard parity: the reveal is not tied to the pointer path ─────
        # `focus/next` is the AI's focus-traversal peer (an INJECTED Tab reaches
        # the focused widget instead of traversing — deliberate, see
        # `ShellCore::handle_named_key`). Walk it until Reply holds focus, then
        # activate with Enter.
        tf.scroll(SCROLL_TAG, to=(0, 0))
        wait_snap(
            tf,
            lambda s: status_of(s) == status_line(len(history), False),
            viewport=WIN,
            desc="back to the top before the keyboard step",
        )
        focused = None
        for _ in range(4):
            focused = tf.request("focus/next").result.get("focused")
            if focused == REPLY_TAG:
                break
        assert_eq(focused, REPLY_TAG, "focus/next reaches the Reply control")
        tf.key(path=REPLY_TAG, name="Enter")
        history.append("reply")
        snap = wait_snap(
            tf,
            lambda s: entry_count(s) == len(history)
            and status_of(s) == status_line(len(history), True),
            viewport=WIN,
            desc="Enter on the focused Reply: same reveal",
        )
        assert_bound_is_measured(tf, snap, "keyboard reply")
        assert_pinned_to_tail(tf, snap, "keyboard reply")
        assert_eq(
            entry_texts(snap), expected_texts(history), "keyboard reply: entry text"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1445 layout-measured tail pin (hello-transcript)", body))
