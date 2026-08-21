#!/usr/bin/env python3
"""hello-content-drag dogfood (R1753 §5.45 §5.35).

The consumer report this round answers, driven rather than described.

A 430x932 phone-shaped list. Every row is a tap target, and the list is
taller than its viewport — how much taller is the example's own test to
state, not this docstring's. Measured on the consumer's build, a press-and-drag
over such a list moved the offset by 0 px and opened the pressed row
instead. `ScrollNode::with_content_drag` is the declaration that fixes
it, and this script shows both halves of the gesture surviving
together:

  1. spawn hello-content-drag
  2. snapshot           -> offset_y == 0, opened is null, cancels == 0
  3. scene/drag LEFT, upward, starting ON A ROW
       -> offset_y > 0        the list panned
       -> opened still null   the row did NOT open
       -> cancels == 1        the pan channel took the press over
  4. scene/click on a row that is visible AFTER that scroll
       -> opened == that row  the tap still works
       -> offset_y unchanged  and the tap scrolled nothing
  5. scene/drag LEFT starting on BARE content (the list's own left
     padding strip, inside the viewport and outside every row)
       -> offset_y grows again  a press with no widget under it pans too
       -> cancels unchanged     because no widget was there to cancel

Steps 3 and 4 are the pair that matters, and neither is enough alone:
a list that always pans passes 3 and fails 4, a list that never pans
does the reverse. `cancels` is the third fact that separates "the row
did not open because the drag worked" from "the row did not open
because presses are broken" — without it, step 3 passes on an app
whose rows are simply dead.

Every coordinate below is READ FROM A SNAPSHOT rather than computed
from the constants in the binary. After step 3 the rows have moved,
so a row centre worked out from row height and index would aim at
where the row used to be.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)


WIN_W = 430
WIN_H = 932

SCROLL_TAG = "set_list_scroll"
ROW_PREFIX = "set_list#"
N_ROWS = 18

# The list's own left padding: inside the scroll viewport, outside every
# row rect (rows are inset 12 px). A press here reaches no widget at all,
# which is the case the bare-content arm exists for.
BARE_X = 6.0


def _offset_y(snap: Any) -> float:
    node = find_by_tag(snap, SCROLL_TAG)
    if node is None:
        raise AssertionError(f"{SCROLL_TAG} not found in paint snapshot")
    value = node.get("offset_y")
    if not isinstance(value, (int, float)):
        raise AssertionError(f"offset_y is not a number: {value!r}")
    return float(value)


def _rect(snap: Any, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    if node is None:
        raise AssertionError(f"{tag} not found in paint snapshot")
    rect = node.get("rect")
    if not isinstance(rect, dict):
        # The KEYS, not the node: a scene node prints its whole subtree, and a
        # snapshot of this list is tens of thousands of characters of failure
        # message that buries the one fact the reader needs.
        raise AssertionError(f"node {tag!r} carries no rect; keys: {sorted(node)}")
    return rect


def _viewport(snap: Any) -> dict:
    """The scroll viewport, in window coordinates.

    A `Scene::Scroll` publishes its clip window as `viewport`, not as `rect` —
    it has two rectangles (the window it shows through and the content it
    holds) and calling either one `rect` would make the pair ambiguous.
    """
    node = find_by_tag(snap, SCROLL_TAG)
    if node is None:
        raise AssertionError(f"{SCROLL_TAG} not found in paint snapshot")
    vp = node.get("viewport")
    if not isinstance(vp, dict):
        raise AssertionError(f"{SCROLL_TAG} carries no viewport: {sorted(node)}")
    return vp


def _fully_visible_row(snap: Any) -> int:
    """The first row wholly inside the list viewport, by index.

    A row's own rect in the snapshot is CONTENT-LOCAL — the coordinate space
    inside the scroll, before the offset is taken off. That is the whole
    reason `scene/click {path: ...}` exists (R51.200): the walker converts
    content-local to absolute so a caller never has to. This function
    therefore does the conversion ONLY to CHOOSE a row, and hands the chosen
    row to the harness BY PATH so the coordinate arithmetic has one home.

    The first draft of this script mixed the two spaces — it read a row's
    content-local centre and clicked it as if it were a window coordinate,
    which after a 200 px scroll aimed two rows away and opened the wrong one.
    The assertion below (`opened == the row we chose`) is what caught it, and
    is kept for that reason: it is a cross-check between this conversion and
    the framework's, not a restatement of it.
    """
    vp = _viewport(snap)
    offset = _offset_y(snap)
    for i in range(N_ROWS):
        r = _rect(snap, f"{ROW_PREFIX}{i}")
        top = vp["y"] + r["y"] - offset
        if top >= vp["y"] and top + r["h"] <= vp["y"] + vp["h"]:
            return i
    raise AssertionError("no row is wholly inside the viewport")


def body() -> None:
    with RpcSubprocess("hello-content-drag") as app:
        snap = app.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        assert_eq(_offset_y(snap), 0.0, "initial offset_y")
        assert_eq(app.query("/external/opened"), None, "initial opened")
        assert_eq(app.query("/external/cancels"), 0, "initial cancels")

        # ── 3. a drag that STARTS ON A ROW pans, and opens nothing ────
        # Both endpoints are given BY PATH, so the harness resolves the
        # content-local rects into window coordinates (R51.200) and this
        # script does no coordinate arithmetic at all. Dragging from a lower
        # row up to a higher row's position is an upward pan by construction.
        app.drag(
            from_path=f"{ROW_PREFIX}8",
            to_path=f"{ROW_PREFIX}2",
            button="left",
            steps=8,
        )

        after_drag = wait_snap(
            app,
            lambda s: _offset_y(s) > 0.0,
            viewport=(WIN_W, WIN_H),
            desc="post-drag offset_y > 0",
        )
        dragged_to = _offset_y(after_drag)
        # ★ The report's first half: this number was 0 on the consumer's build.
        print(f"[demo] drag panned the list to offset_y {dragged_to}")
        # ★ Its second half: the row under the press must not have opened.
        assert_eq(app.query("/external/opened"), None, "opened after a drag")
        assert_eq(app.query("/external/cancels"), 1, "cancels after a drag")

        # ── 4. a tap in place still opens its row ─────────────────────
        tap_row = _fully_visible_row(after_drag)
        app.click(path=f"{ROW_PREFIX}{tap_row}")

        opened = wait_snap(
            app,
            lambda _s: app.query("/external/opened") is not None,
            viewport=(WIN_W, WIN_H),
            desc="a tap opens its row",
        )
        assert_eq(app.query("/external/opened"), tap_row, "the tapped row opened")
        assert_eq(app.query("/external/cancels"), 1, "a tap cancels nothing")
        assert_eq(_offset_y(opened), dragged_to, "a tap scrolls nothing")

        # ── 5. a drag on BARE content pans too ────────────────────────
        # The list's own left padding: inside the viewport, outside every row
        # rect, so this press reaches no widget at all. The viewport's own
        # rect is NOT content-local (nothing encloses it), so these two
        # coordinates are already window coordinates.
        # DOWNWARD, which decreases the offset. Step 3 dragged the list as far
        # as it goes — `ScrollState` clamps at its own maximum — so a second
        # upward pan is a no-op and would prove nothing about this press. The
        # direction with room left is the one that can be observed.
        vp = _viewport(after_drag)
        bare_y = vp["y"] + vp["h"] * 0.25
        app.drag(
            from_at=(vp["x"] + BARE_X, bare_y),
            to_at=(vp["x"] + BARE_X, bare_y + 120.0),
            button="left",
            steps=8,
        )
        after_bare = wait_snap(
            app,
            lambda s: _offset_y(s) < dragged_to,
            viewport=(WIN_W, WIN_H),
            desc="a drag on bare content pans the list back",
        )
        print(f"[demo] bare-content drag panned back to {_offset_y(after_bare)}")
        assert_eq(app.query("/external/cancels"), 1, "no widget was there to cancel")
        assert_eq(app.query("/external/opened"), tap_row, "and nothing new opened")


if __name__ == "__main__":
    sys.exit(run_demo("hello-content-drag press-drag pans, tap opens", body))
