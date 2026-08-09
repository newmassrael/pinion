#!/usr/bin/env python3
"""R1496 §5.27 §5.51 §2#7 §2#2 — a header says who may touch it.

the toolkit reference: `sectionsMovable` and `sectionsClickable`
— both default `false`, both independent of each other, and both serialised by
`write()` (as `movableSections` / `clickableSections`),
which is what `saveState()` hands out. `resizeContentsPrecision` is in that same
stream.

Measured on this very binding before the round, over the real wire:

    query  sections_movable          -> UnknownIntrospectPath
    query  sections_clickable        -> UnknownIntrospectPath
    drag   colhdr#0 -> colhdr#2      -> order [0,1,2,3,4] -> [1,2,0,3,4]
    release in place on colhdr#1     -> sort_indicator none -> 2:ascending

    query  resize_contents_precision -> 1000
    query  state                     -> 10 keys, no resize_contents_precision
    save(1000), set 7, restore       -> 7

The header could be dragged and it could be sorted, and it had no way to say no
to either. Its snapshot — whose own doc calls itself "the peer of the toolkit's
`saveState()`" — carried neither permission, nor the sampling bound the header
already had, so a restore replayed every content-fitted width while dropping the
rule that sized them.

The second half of the round is a correction. R1491 derived the click from the
drop commit: "the permutation came out unchanged, so it was a click". R794 §5.51
is this workspace's click-vs-drag SSOT — it withholds the trailing `PointerUp`
after a gesture that travelled past `DRAG_CLICK_THRESHOLD_PX`, and says in as
many words that no drag source re-derives that per binding. R1491 re-derived it
and got a different answer than the toolkit, which measures the same `startDragDistance`:
picking a column up, changing your mind, and putting it back left the
permutation untouched, so it counted as a click and sorted the column the user
had just decided not to move. That is (D) below.

Moving the click onto the router's release is also what makes (E) possible at
all: a header that opens no drag session has to keep being sortable, and it
cannot if its click is a by-product of a drag.

What this asserts:

  (A) THE RULES ARE READABLE — and this app declares both, the way a toolkit app
      calls `setSectionsMovable` / `setSortingEnabled`. The strip paints them.
  (B) A DRAG MOVES A SECTION — the baseline the permissions gate.
  (C) A CLICK SORTS — a press-release in place, through the same drag primitive.
  (D) A DRAG IS NOT A CLICK — a section dragged well past the threshold and
      dropped back into its own gap moves nothing AND sorts nothing.
  (E) A PINNED HEADER STILL SORTS — `sections_movable` off: the drag does
      nothing, the click still does. The two permissions are independent.
  (F) AN INERT HEADER — `sections_clickable` off too: neither gesture acts, and
      the readout says so rather than looking broken.
  (G) THE PROGRAMMATIC MOVE IS NOT THE GESTURE — `move_section` reorders a
      pinned header, exactly as the toolkit's `moveSection()` does (the R1494 split).
  (H) THE SNAPSHOT CARRIES THEM — save, revoke, restore, and the permissions and
      the sampling bound all come back.
  (I) AN OLDER SNAPSHOT STILL INTERACTS — a state with the three keys removed is
      the pre-R1496 shape, and decodes to the header that shape came from.
  (J) PRESS HERE, RELEASE THERE — the toolkit's same-section rule: neither is activated.

R1497 CORRECTION. This file used to record two "pre-existing router defects"
here — that `scene/click` reached this External not at all, and that a
session-less release left hover un-restored. Both descriptions were wrong, and
they were one defect: `resolve_hover_tag` answered the deepest TAG under the
cursor, so a press whose coordinate fell on a section's own `colhdr_label#<n>`
was dispatched to a tag with no `External` behind it and discarded in silence.
`scene/click {path}` presses a node's rect CENTRE, and the labels of sections 3
and 4 cover theirs — which is why "every click" looked lost while sections 0-2
worked. The missing `PointerEnter` was the enter landing on that same label;
`pointer_up`'s free-release branch is correct to omit `refresh_hover`, because
unlike the DnD and capture branches it never pinned hover and so has nothing to
restore. R1497 moved the resolution to the deepest node that can RECEIVE the
event; `tools/demos/r1497_pointer_target.py` is the witness.

The `_send` seam calls below are therefore no longer forced — they are kept
because they exercise the model's own decode directly, which is a different
thing from the wire arc (B), (C) and (D) drive.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1496_section_permissions.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
LAYOUT_TAG = "colreorder_layout"

HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
BOOT_W = [150, 90, 100, 130, 100]
IDENTITY = list(range(NCOLS))

FLOOR = 40
UNBOUNDED = 2**32 - 1
DEFAULT_SIZE = 100
BOOT_PRECISION = 1000


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    assert node is not None, f"{tag} paints"
    return node["rect"]


def _click_section(tf, visual: int) -> None:
    """A press-release in place — both ends of the drag on one section.

    Deliberately the SAME `scene/drag` primitive (B) and (D) use: if a click
    were a different RPC, the three would prove nothing about each other.
    """
    tf.drag(from_path=f"{HDR}#{visual}", to_path=f"{HDR}#{visual}", steps=4)


def _send(tf, visual: int, event: str):
    """One pointer edge, at the router's own delivery point.

    `dispatch_send` composes exactly this payload and calls exactly this
    method, so driving it is driving the wire — not a back door. R1497
    correction: this was introduced because a following gesture on another
    section "presses nothing", which was true but not for the stated reason —
    the press was being resolved onto a section's own label and discarded (see
    the note at the top of this file). It is kept because a decode assertion
    made at the seam is a different, sharper claim than the whole-arc ones (B),
    (C) and (D) make.
    """
    return tf.invoke("/external/send", f"{visual}:{event}")


def _seam_click(tf, visual: int):
    """A press and a release on one section, at the seam."""
    _send(tf, visual, "PointerDown")
    return _send(tf, visual, "PointerUp")


def _state(tf, **overrides) -> dict:
    """The boot layout as a wire state, with the named fields replaced."""
    base = {
        "order": IDENTITY,
        "sizes": BOOT_W,
        "hidden": [False] * NCOLS,
        "modes": ["interactive"] * NCOLS,
        "sort_indicator": "none",
        "sort_indicator_shown": True,
        "default_section_size": DEFAULT_SIZE,
        "min_section_size": FLOOR,
        "max_section_size": UNBOUNDED,
        "cascading_section_resizes": False,
        "sections_movable": True,
        "sections_clickable": True,
        "resize_contents_precision": BOOT_PRECISION,
    }
    base.update(overrides)
    return base


def _reset(tf, **overrides) -> None:
    """Back to the boot layout through the restore path itself."""
    want = _state(tf, **overrides)
    tf.intervene("/external/state", want)
    wait_until(
        lambda: _h(tf, "order") == want["order"]
        and _h(tf, "sort_indicator") == want["sort_indicator"]
        and _h(tf, "sections_movable") == want["sections_movable"]
        and _h(tf, "sections_clickable") == want["sections_clickable"],
        desc="layout reset",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        wait_until(
            lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
            desc="the strip paints",
        )

        # ── (A) the rules are readable, and this app declares both ────
        assert_eq(_h(tf, "sections_movable"), True,
                  "the app declared its header movable")                   # 1
        assert_eq(_h(tf, "sections_clickable"), True,
                  "and clickable, as setSortingEnabled does in the toolkit")        # 2
        assert "| allows move+click" in _readout(tf), \
            f"the strip paints what it allows: {_readout(tf)}"             # 3
        state = _h(tf, "state")
        assert_eq(state["sections_movable"], True,
                  "saveState carries the permission the toolkit calls movableSections") # 4
        assert_eq(state["sections_clickable"], True,
                  "and the one it calls clickableSections")                # 5
        assert_eq(state["resize_contents_precision"], BOOT_PRECISION,
                  "and resizeContentsPrecision, measured absent before")   # 6

        # ── (B) a drag moves a section ────────────────────────────────
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#2", steps=6)
        wait_until(lambda: _h(tf, "order") == [1, 2, 0, 3, 4],
                   desc="the drag commits")                                # 7
        assert_eq(_h(tf, "sort_indicator"), "none",
                  "a moved drag sorted nothing")                           # 8

        # ── (C) a click sorts ─────────────────────────────────────────
        _reset(tf)
        _click_section(tf, 1)
        wait_until(lambda: _h(tf, "sort_indicator") == "1:ascending",
                   desc="a release in place sorts the section it pressed") # 9
        assert_eq(_h(tf, "order"), IDENTITY, "and moved nothing")         # 10
        _click_section(tf, 1)
        wait_until(lambda: _h(tf, "sort_indicator") == "1:descending",
                   desc="clicking again reverses it")                     # 11

        # A second click on one section arrives with a `DoubleClick` between its
        # `PointerDown` and its `PointerUp` — measured on this router. Driven at
        # the seam rather than by clicking twice quickly, because whether the
        # arc produces that edge depends on the double-click WINDOW, and an
        # assertion whose discrimination depends on wall-clock timing is a flake
        # waiting to happen. The first draft of `handle_send` dropped the press
        # on any event it did not recognise, so this edge silently ate every
        # other click.
        _send(tf, 1, "PointerDown")
        _send(tf, 1, "DoubleClick")
        # The binding answers the indicator its click produced — here the third
        # state of the toolkit's cycle, the STRING "none", which is not the `None`
        # an unclicked release answers.
        assert_eq(_send(tf, 1, "PointerUp"), "none",
                  "a second notification about a live press does not end it: "
                  "the release still clicked, and the cycle reached unsorted") # 12
        wait_until(lambda: _h(tf, "sort_indicator") == "none",
                   desc="and the header published that")                  # 13

        # ── (D) a drag is not a click, even when it lands where it began ──
        # R1491's rule was "the permutation came out unchanged": under it, this
        # gesture sorted. The cursor travels most of a 150px section — far past
        # DRAG_CLICK_THRESHOLD_PX (4px) — and is released on the LEFT half of
        # the section it started on, so the drop slot is that section's own gap.
        _reset(tf)
        rect = _rect(tf, f"{HDR}#0")
        tf.drag(
            from_path=f"{HDR}#0",
            to_at=(rect["x"] + 4, rect["y"] + rect["h"] / 2),
            steps=8,
        )
        wait_until(lambda: _h(tf, "preview") is None, desc="the drag ended") # 14
        assert_eq(_h(tf, "order"), IDENTITY,
                  "the section went back where it came from")             # 15
        assert_eq(_h(tf, "sort_indicator"), "none",
                  "and changing your mind is not a sort")                 # 16

        # ── (E) a pinned header still sorts ───────────────────────────
        _reset(tf, sections_movable=False)
        assert "| allows click" in _readout(tf), \
            f"the readout drops the half it revoked: {_readout(tf)}"      # 17
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#2", steps=6)
        wait_until(lambda: _h(tf, "preview") is None, desc="the gesture ended") # 18
        assert_eq(_h(tf, "order"), IDENTITY,
                  "a pinned header refused the drag")                     # 19
        assert_eq(_h(tf, "sort_indicator"), "none",
                  "and it did not become a click either: the release landed on "
                  "another section, and the toolkit's rule is that a click is a press "
                  "and a release on the SAME one")                        # 20
        assert_eq(_seam_click(tf, 3), "3:ascending",
                  "but a press-release still sorts, which IS the independence "
                  "claim: no drag session, and the click is not a by-product "
                  "of one")                                               # 21
        wait_until(lambda: _h(tf, "sort_indicator") == "3:ascending",
                   desc="and the header published it")                    # 22
        assert_eq(_h(tf, "order"), IDENTITY, "with nothing moved")        # 23

        # ── (F) an inert header ───────────────────────────────────────
        _reset(tf, sections_movable=False, sections_clickable=False)
        assert "| allows -" in _readout(tf), \
            f"the readout says it allows nothing: {_readout(tf)}"         # 24
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#2", steps=6)
        wait_until(lambda: _h(tf, "preview") is None, desc="the gesture ended") # 25
        assert_eq(_h(tf, "order"), IDENTITY, "nothing moved")             # 26
        assert_eq(_h(tf, "sort_indicator"), "none", "and nothing sorted") # 27
        assert_eq(_seam_click(tf, 2), None,
                  "a click on an unclickable header is not a click")      # 28
        assert_eq(_h(tf, "sort_indicator"), "none", "so nothing sorted")  # 29

        # ── (G) the programmatic move is not the gesture ──────────────
        _reset(tf, sections_movable=False)
        assert_eq(
            tf.invoke("/external/move_section", "0:2"),
            [1, 2, 0, 3, 4],
            "moveSection reorders a header the user cannot drag, as in the toolkit",
        )                                                                 # 30
        assert_eq(_h(tf, "sections_movable"), False,
                  "and the rule it ignored is still the rule")            # 31

        # ── (H) the snapshot carries them ─────────────────────────────
        _reset(tf)
        tf.intervene("/external/resize_contents_precision", 250)
        wait_until(lambda: _h(tf, "resize_contents_precision") == 250,
                   desc="the sampling bound moved")                       # 32
        saved = _h(tf, "state")
        tf.intervene("/external/sections_movable", False)
        tf.intervene("/external/sections_clickable", False)
        tf.intervene("/external/resize_contents_precision", 7)
        wait_until(lambda: _h(tf, "resize_contents_precision") == 7,
                   desc="drifted away from the snapshot")                 # 33
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "sections_movable") is True,
                   desc="the restore replays the permission")             # 34
        assert_eq(_h(tf, "sections_clickable"), True, "and the other one") # 35
        assert_eq(_h(tf, "resize_contents_precision"), 250,
                  "and the sampling bound the entry measurement lost")    # 36

        # ── (I) an older snapshot still interacts ─────────────────────
        older = dict(saved)
        for key in ("sections_movable", "sections_clickable",
                    "resize_contents_precision"):
            older.pop(key)
        tf.intervene("/external/sections_movable", False)
        wait_until(lambda: _h(tf, "sections_movable") is False, desc="revoked") # 37
        tf.intervene("/external/state", older)
        wait_until(lambda: _h(tf, "sections_movable") is True,
                   desc="a pre-R1496 snapshot restores the header it came from") # 38
        assert_eq(_h(tf, "sections_clickable"), True,
                  "not the toolkit's `false`: what that older header actually did") # 39
        assert_eq(_h(tf, "resize_contents_precision"), BOOT_PRECISION,
                  "the bound falls back to the constant, like the other scalars") # 40

        # ── (J) press here, release there ───────────────────────────── the
        # toolkit's `logicalIndexAt(pos) == d->pressed`. Driven at the seam because the arc cannot express it:
        # a gesture that travels from one section to another travels past
        # DRAG_CLICK_THRESHOLD_PX, and R794 then withholds the release entirely
        # — so the router never asks this question, and a test that used the
        # arc would pass with the rule deleted.
        _reset(tf)
        _send(tf, 0, "PointerDown")
        assert_eq(_send(tf, 3, "PointerUp"), None,
                  "a press that ended on another section is neither's click") # 41
        assert_eq(_h(tf, "sort_indicator"), "none", "so nothing sorted")  # 42
        assert_eq(_seam_click(tf, 3), "3:ascending",
                  "and the abandoned press did not survive to be redeemed") # 43

        # A press that wanders off the strip is over.
        _send(tf, 1, "PointerDown")
        _send(tf, 1, "PointerLeave")
        assert_eq(_send(tf, 1, "PointerUp"), None,
                  "a press ended by a leave activates nothing")           # 44

        # A permission is a boolean, and a client that sent something else is
        # told so rather than having it coerced.
        assert_rpc_error(
            lambda: tf.intervene("/external/sections_movable", 1),
            data="InterveneTypeMismatch",
        )                                                                 # 45
        assert_eq(_h(tf, "sections_movable"), True, "the refusal changed nothing") # 46

        # A permission is a boolean, and a client that sent something else is
        # told so rather than having it coerced.


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1496 §5.27 §5.51 §2#7 — header view permissions: movable, clickable",
        body,
    ))
