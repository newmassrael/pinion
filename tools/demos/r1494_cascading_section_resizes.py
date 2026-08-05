#!/usr/bin/env python3
"""R1494 §5.27 §2#7 §2#2 — a resize is paid for by the sections after it.

Qt reference: `QHeaderView::cascadingSectionResizes` — "whether interactive
resizing will be cascaded to the following sections", default `false`, and it
governs *interactive* resizing only: `resizeSection()` never cascades.

Measured on this very binding before the round, over the real wire:

    query  cascading_section_resizes -> UnknownIntrospectPath
    invoke resize_section "0:300"    -> 300
    query  visible_total             -> 720     (available_width is 640)

    query  min_section_size          -> 40
    invoke resize_section "1:10"     -> 40
    query  visible_total             -> 520     (it was 570)

A resize was not paid for by anything. Growing a section pushed the row 80px
past the viewport it was told to fill, and shrinking one below the floor simply
stopped, with the travel the user asked for going nowhere. There was no rule
saying which of those should happen, and therefore no way to ask for the other.

The rule is per header, readable, writable, and carried by `saveState()` — Qt
carries it too. With it on, growing a section squeezes the ones AFTER it, in
visual order, each down to the floor; shrinking it hands that space back,
most-recently-squeezed first. What each follower held before it paid is
remembered, because a cascade that cannot be undone is not a drag: pulling a
section out and back must leave the row exactly where it started.

Only `Interactive`, visible sections pay. A `Fixed` section is fixed against a
neighbour's drag as much as against its own; a `Stretch` or `ResizeToContents`
section derives its size and has no stored width to give; a hidden section is
painted nowhere.

This binding has no pointer grabber — `column_resize_externals` lives in
`hello-grid-hscroll`, over a `ColumnWidths` with no layout — so its `[` / `]`
keystroke IS its interactive resize, and that is what this drives. Before this
round that gesture was gated like an interactive resize (it refuses a `Fixed`
section) while writing like a programmatic one.

What this asserts:

  (A) THE RULE IS READABLE AND OFF — Qt's default, and the measurement above
      replayed: with it off the row still grows past its viewport.
  (B) THE FOLLOWERS PAY — the same call, rule on, leaves `visible_total`
      exactly where it was.
  (C) OUT AND BACK — a grow then a shrink returns every section to the width it
      started at, which is what the memory is for.
  (D) WHO PAYS — `Fixed`, `Stretch` and hidden followers are skipped, and the
      next eligible one pays instead.
  (E) SPENT — when every follower is at the floor the section still grows and
      `visible_total` reports the row that is actually painted, rather than
      refusing a resize with nothing to say why.
  (F) TWO METHODS, ON PURPOSE — `resize_section` does not cascade even with the
      rule on, because in Qt the property governs interactive resizing only.
  (G) THE GESTURE ENDS — a different anchor, and a programmatic write, both
      drop the debt rather than repaying it out of unrelated travel.
  (H) THE PAINTED RULE — the readout names it, and the same keystroke visibly
      does two different things.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1494_cascading_section_resizes.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
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

FLOOR = 40
UNBOUNDED = 2**32 - 1
DEFAULT_SIZE = 100
BOOT_TOTAL = sum(BOOT_W)


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _reset(tf, *, cascading: bool = False) -> None:
    """Back to the boot layout, with the rule set explicitly."""
    tf.intervene(
        "/external/state",
        {
            "order": IDENTITY,
            "sizes": BOOT_W,
            "hidden": [False] * NCOLS,
            "modes": ["interactive"] * NCOLS,
            "sort_indicator": "none",
            "sort_indicator_shown": True,
            "default_section_size": DEFAULT_SIZE,
            "min_section_size": FLOOR,
            "max_section_size": UNBOUNDED,
            "cascading_section_resizes": cascading,
        },
    )
    wait_until(
        lambda: _h(tf, "sizes") == BOOT_W
        and _h(tf, "cascading_section_resizes") == cascading,
        desc=f"layout reset (cascading={cascading})",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) the rule is readable, and off ─────────────────────────
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                   desc="the strip paints")
        assert_eq(_h(tf, "cascading_section_resizes"), False,
                  "off by default, as in Qt")                                    # 1
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL, "the boot row")           # 2
        assert_eq(_h(tf, "available_width"), AVAILABLE_W, "and its viewport")    # 3

        # The measurement this round entered on, replayed.
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:300"), 300,
                  "the resize applies")                                          # 4
        assert_eq(_h(tf, "sizes"), [300, 90, 100, 130, 100],
                  "and nobody else paid for it")                                 # 5
        assert_eq(_h(tf, "visible_total"), 720,
                  "so the row is now 80px past the viewport it was told to fill")  # 6

        # ── (B) the followers pay ─────────────────────────────────────
        _reset(tf, cascading=True)
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:300"), 300,
                  "the same call, the same applied width")                       # 7
        assert_eq(_h(tf, "sizes"), [300, 40, 40, 90, 100],
                  "the nearest followers paid, each down to the floor, in order")  # 8
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL,
                  "and the row is exactly as wide as it was")                    # 9
        assert_eq(_h(tf, "section_sizes"), _h(tf, "sizes"),
                  "all interactive, so stored and effective still agree")        # 10

        # ── (C) out and back ──────────────────────────────────────────
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:150"), 150,
                  "drag it back to where it started")                            # 11
        assert_eq(_h(tf, "sizes"), BOOT_W,
                  "and every section is at the width it began at")               # 12
        # Half way back lets the most-recently-squeezed section go first.
        tf.invoke("/external/interactive_resize_section", "0:300")
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:260"), 260,
                  "part of the way back")                                        # 13
        assert_eq(_h(tf, "sizes"), [260, 40, 40, 130, 100],
                  "the last section squeezed is the first let go")               # 14

        # ── (D) who pays ──────────────────────────────────────────────
        _reset(tf, cascading=True)
        tf.invoke("/external/set_resize_mode", "1:fixed")
        wait_until(lambda: _h(tf, "resize_mode.1") == "fixed", desc="Type is fixed")
        tf.invoke("/external/interactive_resize_section", "0:200")
        assert_eq(_h(tf, "sizes"), [200, 90, 50, 130, 100],
                  "the Fixed follower was skipped; the next one paid instead")   # 15

        _reset(tf, cascading=True)
        tf.invoke("/external/set_section_hidden", "1:true")
        wait_until(lambda: _h(tf, "hidden")[1], desc="Type is hidden")
        tf.invoke("/external/interactive_resize_section", "0:200")
        assert_eq(_h(tf, "sizes"), [200, 90, 50, 130, 100],
                  "and a hidden follower keeps its width too")                   # 16
        assert_eq(_h(tf, "hidden_count"), 1, "it is still hidden")               # 17

        _reset(tf, cascading=True)
        tf.invoke("/external/set_resize_mode", "1:stretch")
        tf.intervene("/external/available_width", AVAILABLE_W)
        wait_until(lambda: _h(tf, "resize_mode.1") == "stretch", desc="Type stretches")
        tf.invoke("/external/interactive_resize_section", "0:200")
        assert_eq(_h(tf, "sizes"), [200, 90, 50, 130, 100],
                  "a Stretch follower derives its size and has none to give")    # 18

        # ── (E) spent ─────────────────────────────────────────────────
        _reset(tf, cascading=True)
        slack = sum(w - FLOOR for w in BOOT_W[1:])
        assert_eq(slack, 260, "what the four followers can give between them")   # 19
        assert_eq(
            tf.invoke("/external/interactive_resize_section",
                      f"0:{BOOT_W[0] + slack}"),
            BOOT_W[0] + slack, "spend them exactly")                             # 20
        assert_eq(_h(tf, "sizes"), [410, 40, 40, 40, 40],
                  "every follower at the floor")                                 # 21
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL, "and the row has not grown")  # 22
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:500"), 500,
                  "one step further, and the section still gets what it asked")  # 23
        assert_eq(_h(tf, "sizes"), [500, 40, 40, 40, 40],
                  "nobody could pay, so it grew alone")                          # 24
        assert_eq(_h(tf, "visible_total"), 660,
                  "and visible_total reports the row that is actually painted")  # 25

        # ── (F) two methods, on purpose ───────────────────────────────
        _reset(tf, cascading=True)
        assert_eq(tf.invoke("/external/resize_section", "0:300"), 300,
                  "the programmatic resize applies")                             # 26
        assert_eq(_h(tf, "sizes"), [300, 90, 100, 130, 100],
                  "and does NOT cascade, rule on or not — Qt's own split")       # 27
        assert_eq(_h(tf, "visible_total"), 720, "so this one does grow the row")  # 28
        assert_action_refused(
            lambda: tf.invoke("/external/interactive_resize_section", "9:200"),
            saying="no section 9 in this header",
        )                                                                        # 29
        assert_action_refused(
            lambda: tf.invoke("/external/interactive_resize_section", "nonsense"),
            saying='malformed argument "nonsense"',
        )                                                                        # 30

        # ── (G) the gesture ends ──────────────────────────────────────
        _reset(tf, cascading=True)
        tf.invoke("/external/interactive_resize_section", "0:300")
        assert_eq(_h(tf, "sizes"), [300, 40, 40, 90, 100], "a debt is owed")      # 31
        # Shrink a DIFFERENT section, so what is asserted is the debt and not a
        # second cascade of its own.
        tf.invoke("/external/interactive_resize_section", "3:70")
        assert_eq(_h(tf, "sizes"), [300, 40, 40, 70, 100],
                  "a different anchor is a different gesture: only it moved")    # 32
        tf.invoke("/external/interactive_resize_section", "0:150")
        assert_eq(_h(tf, "sizes"), [150, 40, 40, 70, 100],
                  "and the old debt did not follow the new gesture")             # 33

        _reset(tf, cascading=True)
        tf.invoke("/external/interactive_resize_section", "0:300")
        tf.invoke("/external/resize_section", "1:90")
        tf.invoke("/external/interactive_resize_section", "0:150")
        assert_eq(_h(tf, "sizes"), [150, 90, 40, 90, 100],
                  "a programmatic write ends the gesture, so nothing stale "
                  "overwrote the width it just set")                             # 34

        # Withdrawing the rule withdraws the debt it created. Repaying after
        # the rule is gone would move sections out of a gesture that is no
        # longer cascading.
        _reset(tf, cascading=True)
        tf.invoke("/external/interactive_resize_section", "0:300")
        assert_eq(_h(tf, "sizes"), [300, 40, 40, 90, 100], "a debt is owed again")
        tf.intervene("/external/cascading_section_resizes", False)
        wait_until(lambda: _h(tf, "cascading_section_resizes") is False,
                   desc="the rule is withdrawn")
        tf.intervene("/external/cascading_section_resizes", True)
        wait_until(lambda: _h(tf, "cascading_section_resizes"), desc="and re-armed")
        tf.invoke("/external/interactive_resize_section", "0:150")
        assert_eq(_h(tf, "sizes"), [150, 40, 40, 90, 100],
                  "nobody was repaid by a rule that had been withdrawn")

        # ── (H) the painted rule ──────────────────────────────────────
        _reset(tf, cascading=False)
        assert "| cascade off |" in _readout(tf), \
            f"the readout names the rule: {_readout(tf)}"                        # 35
        tf.intervene("/external/cascading_section_resizes", True)
        wait_until(lambda: "| cascade on |" in _readout(tf),
                   desc="and names it when it changes")                          # 36
        assert_rpc_error(
            lambda: tf.intervene("/external/cascading_section_resizes", 1),
            data="InterveneTypeMismatch",
        )                                                                        # 37

        # The saveState peer carries it, so a restore replays the rule.
        saved = _h(tf, "state")
        assert_eq(saved["cascading_section_resizes"], True,
                  "the snapshot carries Qt's property")                          # 38
        _reset(tf, cascading=False)
        assert_eq(_h(tf, "cascading_section_resizes"), False, "a header without it")  # 39
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "cascading_section_resizes"),
                   desc="the restore brought the rule back")                     # 40
        assert_eq(_h(tf, "state"), saved, "the whole snapshot came back")         # 41


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1494 §5.27 §2#7 — QHeaderView cascadingSectionResizes: who pays for a resize",
        body,
    ))
