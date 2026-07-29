#!/usr/bin/env python3
"""R1492 §5.27 §2#7 §2#2 — a section says what its size is allowed to be.

Qt reference: `QHeaderView::setMinimumSectionSize` / `setMaximumSectionSize`,
both runtime setters, both governing every way a section acquires a size.

Measured on this very binding before the round, over the real wire:

    query min_width / max_width / min_section_size / max_section_size
        -> UnknownIntrospectPath  (all four)
    invoke resize_section "0:99999"   -> 99999      (window is 700px wide)
    invoke resize_section "2:300"     -> 40         (mode = stretch)
    invoke resize_section "0:5"       -> 40         (mode = interactive)

Three separate defects in five calls. There was **no ceiling at all**: a section
could be set to 99999 in a 700px window, and a `ResizeToContents` section took
its content hint however long one pathological cell was. The floor existed but
was fixed at construction and **not readable from the header**, so the rule that
shaped every resize was invisible. And because it was invisible, the last two
lines above — a width clamped UP to the floor, and a width the stretch mode
derived — were byte-identical answers with unrelated causes.

The fix is not a new disclosure channel. It is that the rule became legible: with
`min_section_size`, `max_section_size` and the already-readable
`resize_mode.<logical>`, a client names the cause itself. Qt has no such channel
either, and does not need one.

The bound is applied through ONE clamp (`ColumnWidths::clamp`) rather than
repeated per size path, because a section gets a size three ways — a stored
width, a content hint, a stretch share — and each previously wrote `.max(min)`
on its own. A ceiling that only one of the three honoured would make the row
fill differently depending on a mode the ceiling has nothing to do with.

What this asserts:

  (A) BOOT — both bounds are readable, and the default ceiling SAYS it is
      unbounded rather than omitting the slot.
  (B) THE CEILING EXISTS — the exact call that returned 99999 before this round
      now returns the ceiling, and the painted rect agrees.
  (C) EVERY PATH — stored width, content hint (`ResizeToContents`) and stretch
      share are all clamped by the same ceiling. This is the assertion that a
      per-path `.max(min)` implementation fails.
  (D) TWO IDENTICAL SIZES, TOLD APART — the measurement above, replayed: both
      calls still answer the same number, and the readable bounds + mode now
      name which rule shaped each.
  (E) BOUNDS ARE SETTINGS — moving one re-clamps widths that already exist, and
      the readable snapshot records what the header will actually paint.
  (F) NEITHER DOOR OUTRANKS THE OTHER — the bounds are writable, and a refusal
      is typed.
  (G) THE PAINTED RULE — the on-screen readout shows both ends, so a user who
      drags into a stop can see why.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1492_section_size_bounds.py
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
AVAILABLE_W = 640
GUTTER = 2

# `DEFAULT_MIN_COL_WIDTH` / `DEFAULT_MAX_COL_WIDTH` — the binding's constants.
FLOOR = 40
UNBOUNDED = 2**32 - 1


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    return None if node is None else node["rect"]


def _visual_of(tf, logical: int) -> int:
    return _h(tf, f"visual_index.{logical}")


def _reset(tf) -> None:
    """Back to the boot layout, bounds included."""
    tf.intervene("/external/max_section_size", UNBOUNDED)
    tf.intervene("/external/min_section_size", FLOOR)
    tf.intervene(
        "/external/state",
        {
            "order": IDENTITY,
            "sizes": BOOT_W,
            "hidden": [False] * NCOLS,
            "modes": ["interactive"] * NCOLS,
            "sort_indicator": "none",
            "sort_indicator_shown": True,
        },
    )
    wait_until(
        lambda: _h(tf, "sizes") == BOOT_W and _h(tf, "max_section_size") == UNBOUNDED,
        desc="layout reset",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) the rule is legible ───────────────────────────────────
        wait_until(lambda: _rect(tf, f"{HDR}#0") is not None, desc="the strip paints")
        assert_eq(_h(tf, "min_section_size"), FLOOR,
                  "the floor that shapes every resize is READABLE")               # 1
        assert_eq(_h(tf, "max_section_size"), UNBOUNDED,
                  "and the ceiling says 'none' rather than omitting the slot")    # 2
        assert_eq(_h(tf, "sizes"), BOOT_W, "boot sizes")                          # 3

        # ── (B) there is a ceiling now ────────────────────────────────
        # The exact call that answered 99999 before this round.
        assert_eq(tf.invoke("/external/resize_section", "0:99999"), 99_999,
                  "unbounded by default, so the huge width still applies")        # 4
        tf.intervene("/external/max_section_size", 180)
        wait_until(lambda: _h(tf, "section_size.0") == 180,
                   desc="the new ceiling reaches the width already stored")       # 5
        assert_eq(tf.invoke("/external/resize_section", "0:99999"), 180,
                  "and the same call now answers the ceiling")                    # 6
        v0 = _visual_of(tf, 0)
        assert_eq(_rect(tf, f"{HDR}#{v0}")["w"], 180 - GUTTER,
                  "the PAINT agrees — this is not a model-only bound")            # 7

        # ── (C) every size path, not just the stored one ──────────────
        # A per-path `.max(min)` implementation passes (B) and fails here.
        tf.invoke("/external/set_resize_mode", "1:resize_to_contents")
        tf.intervene("/external/max_section_size", 60)
        wait_until(lambda: _h(tf, "section_size.1") == 60,
                   desc="a content-fitted section obeys the ceiling")             # 8
        assert_eq(_h(tf, "section_size.0"), 60, "so does a stored width")         # 9
        tf.invoke("/external/set_resize_mode", "2:stretch")
        assert_eq(_h(tf, "section_size.2"), 60,
                  "and so does a stretch share, which no stored width produced")  # 10
        assert_eq(_h(tf, "visible_widths"), [60] * NCOLS,
                  "the whole row is bounded, by one rule")                        # 11
        # The row consequently does NOT fill the viewport, and reports the truth
        # rather than a number the grid is not painting.
        assert _h(tf, "visible_total") < AVAILABLE_W, \
            f"a bounded row cannot fill an unbounded viewport: {_h(tf, 'visible_total')}"  # 12

        # ── (D) the measurement, replayed and now explicable ──────────
        # Replayed in the order the probe ran it: section 0 is made huge first,
        # so the stretch remainder collapses. That ordering is load-bearing —
        # with a normal-sized neighbour the share is 280 and the two answers
        # never collide, which is precisely why the collision was easy to miss.
        _reset(tf)
        tf.invoke("/external/resize_section", "0:99999")
        tf.invoke("/external/set_resize_mode", "2:stretch")
        wait_until(lambda: _h(tf, "resize_mode.2") == "stretch", desc="Size stretches")
        derived = tf.invoke("/external/resize_section", "2:300")
        clamped = tf.invoke("/external/resize_section", "0:5")
        assert_eq(derived, FLOOR, "asked 300, got 40 — as before")                # 13
        assert_eq(clamped, FLOOR, "asked 5, got 40 — also as before")             # 14
        assert_eq(clamped, derived, "the two answers are STILL identical")        # 15
        # And now each is nameable from readable state — which needs BOTH facts,
        # because here the floor is what produced each number and only the mode
        # says what it was applied to. Cause 1: a request below the floor.
        assert 5 < _h(tf, "min_section_size"), \
            "5 is below the floor"                                                # 16
        assert_eq(_h(tf, "resize_mode.0"), "interactive",
                  "and an interactive section STORES its request, so the answer \
is that request clamped")                                                          # 17
        # Cause 2: the request was inside the bounds, so nothing clamped the
        # REQUEST. The mode derived a share instead, and the share hit the floor.
        assert _h(tf, "min_section_size") < 300 < _h(tf, "max_section_size"), \
            "300 sits inside the bounds, so no bound touched the request"         # 18
        assert_eq(_h(tf, "resize_mode.2"), "stretch",
                  "its MODE derives the size — a different cause for one number") # 19
        # The stored value was kept, which is the third fact that separates
        # "clamped" from "derived": switch the mode back and it reappears.
        tf.invoke("/external/set_resize_mode", "2:interactive")
        wait_until(lambda: _h(tf, "section_size.2") == 300,
                   desc="the derived section had stored 300 all along")           # 20

        # ── (E) bounds are settings, and the snapshot tells the truth ─
        _reset(tf)
        tf.intervene("/external/min_section_size", 120)
        wait_until(lambda: _h(tf, "sizes") == [150, 120, 120, 130, 120],
                   desc="raising the floor lifts the widths already stored")      # 21
        state = _h(tf, "state")
        assert_eq(state["sizes"], [150, 120, 120, 130, 120],
                  "and the readable snapshot records what will be painted")       # 22
        assert_eq(_h(tf, "min_section_size"), 120, "the floor moved")             # 23
        # The two bounds are set independently, so one can be pushed past the
        # other; the pair carries the other rather than leaving an empty range,
        # and SAYS so on read-back.
        tf.intervene("/external/max_section_size", 60)
        wait_until(lambda: _h(tf, "max_section_size") == 60, desc="ceiling down")  # 24
        assert_eq(_h(tf, "min_section_size"), 60,
                  "the floor came down with it rather than inverting the pair")   # 25
        assert_eq(_h(tf, "visible_widths"), [60] * NCOLS,
                  "and the row resolves under the collapsed range")               # 26

        # ── (F) both doors, and typed refusals ────────────────────────
        _reset(tf)
        assert_rpc_error(
            lambda: tf.intervene("/external/max_section_size", "wide"),
            data="InterveneTypeMismatch",
        )                                                                          # 27
        assert_rpc_error(
            lambda: tf.intervene("/external/min_section_size", "narrow"),
            data="InterveneTypeMismatch",
        )                                                                          # 28
        assert_eq(_h(tf, "min_section_size"), FLOOR, "no refusal moved a bound")  # 29
        assert_eq(_h(tf, "max_section_size"), UNBOUNDED, "neither of them")       # 30
        assert_eq(_h(tf, "sizes"), BOOT_W, "and no width moved either")           # 31

        # ── (G) the painted rule ──────────────────────────────────────
        # A clamp whose rule is invisible reads as a bug: "I dragged and it
        # stopped" needs a visible cause.
        readout = find_by_tag(_paint(tf), LAYOUT_TAG)["content"]
        # R1496 appended the permissions to this row, so the bounds are no
        # longer its tail; they are still one contiguous claim, which is what
        # this checks.
        assert "| bounds 40..- |" in readout, \
            f"the unbounded default reads as '-', not as 4294967295: {readout}"   # 32
        tf.intervene("/external/max_section_size", 130)
        wait_until(lambda: "| bounds 40..130 |"
                   in find_by_tag(_paint(tf), LAYOUT_TAG)["content"],
                   desc="the painted rule names both ends")                       # 33
        assert_eq(_h(tf, "sizes"), [130, 90, 100, 130, 100],
                  "and the sections over the ceiling came down to it")            # 34


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1492 §5.27 §2#7 — QHeaderView section size bounds: floor, ceiling, rule",
        body,
    ))
