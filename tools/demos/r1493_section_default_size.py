#!/usr/bin/env python3
"""R1493 §5.27 §2#7 §2#2 — the size a section was given, and the size it has.

the toolkit reference: `defaultSectionSize` / `setDefaultSectionSize` /
`resetDefaultSectionSize`, and `sectionSize()` — the effective size, which is
what the toolkit's accessor means and what `saveState()` deliberately does not carry.

Measured on this very binding before the round, over the real wire:

    query default_section_size        -> UnknownIntrospectPath
    invoke reset_default_section_size -> UnknownInvokePath

    # all-stretch header, 640px of viewport to divide:
    query sizes          -> [150, 90, 100, 130, 100]   <- the readable plural
    query section_size.0 -> 128                        <- the singular
    PAINTED rect of 0    -> 126                        <- ground truth

Two defects. The first is the missing one: a header could not say what size a
section takes when nothing determined it, so it could not be reset either, and a
section that had never been sized was indistinguishable from one deliberately
set to the floor.

The second is what the first would have made worse. A section has a size it was
**given** — stored, restorable, what `saveState()` replays — and a size it
**has**, resolved through its `SectionResizeMode`. Under `Interactive` and
`Fixed` the two are equal, which is why one name sufficed for 42 rounds. Under
`Stretch` and `ResizeToContents` they are not, and the only logical-keyed plural
on the wire was the stored one: `sizes` answered `[150, 90, …]` for a row
painting `[128, 128, …]`. `resize_section`'s own doc already promises the
opposite — "the return is the size the section actually has, so a client is
never told a number the grid is not painting" — so the single-section write path
kept the rule and the plural read path broke it.

The two are one round because `setDefaultSectionSize` writes STORED widths in
bulk. Landing it alone would have made `sizes` confidently report a number
nothing on screen had, for every section at once.

the toolkit keeps one number per section and re-derives it on relayout, so it needs no
such pair. pinion keeps the size you asked for across a mode switch — a strictly
larger capability — and having kept two numbers must say which one it is
handing you. Hence `section_sizes` beside `sizes`, and a readout that shows the
painted one first.

What this asserts:

  (A) BOOT — the default is readable, and the two plurals agree while the modes
      are `Interactive`, which is the state in which the old single name was
      not yet wrong.
  (B) THE PLURALS PART — the exact measurement above, replayed: under a stretch
      row `sizes` and `section_sizes` differ, and `section_sizes` is the one the
      painted rects agree with.
  (C) SINGULAR AND PLURAL CANNOT DISAGREE — the plural is where the singular
      reads from, asserted section by section.
  (D) THE DEFAULT GOVERNS, AND SPARES THE HIDDEN — the toolkit's bulk rule, including
      the section a user cannot see it happen to.
  (E) RESET WITHOUT NAMING THE CONSTANT — `resetDefaultSectionSize`, and it
      answers the row it produced rather than the number it wrote.
  (F) THE RULES TRAVEL WITH THE OUTCOMES — `saveState()` carries the default and
      both bounds now; a restore replays the rule, not only its result, and the
      restored header refuses what the saved one refused.
  (G) A RESTORE IS NOT ORDER-DEPENDENT — an inverted bound pair is refused
      whole, and a wide layout restored into a narrow header keeps its widths
      because the incoming bounds land before the widths they shape.
  (H) THE PAINTED RULE — the readout leads with the size on screen, names the
      stored one only when they differ, and names the default.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1493_section_default_size.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_out_of_range,
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

# The binding's constants.
FLOOR = 40
UNBOUNDED = 2**32 - 1
DEFAULT_SIZE = 100


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _painted_widths(tf) -> list[int]:
    """The header rects the strip actually paints, in visual order."""
    snap = _paint(tf)
    out = []
    for visual in range(NCOLS):
        node = find_by_tag(snap, f"{HDR}#{visual}")
        if node is not None:
            out.append(round(node["rect"]["w"]))
    return out


def _reset(tf) -> None:
    """Back to the boot layout — modes, bounds, default and all."""
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
            "default_section_size": DEFAULT_SIZE,
            "min_section_size": FLOOR,
            "max_section_size": UNBOUNDED,
        },
    )
    wait_until(
        lambda: _h(tf, "sizes") == BOOT_W
        and _h(tf, "default_section_size") == DEFAULT_SIZE
        and _h(tf, "max_section_size") == UNBOUNDED,
        desc="layout reset",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) boot: one name was enough, here ───────────────────────
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                   desc="the strip paints")
        assert_eq(_h(tf, "default_section_size"), DEFAULT_SIZE,
                  "the size a section takes when nothing determined it")        # 1
        assert_eq(_h(tf, "sizes"), BOOT_W, "the stored plural")                 # 2
        assert_eq(_h(tf, "section_sizes"), BOOT_W,
                  "and the effective plural, which agrees while all interactive")  # 3
        assert_eq(_h(tf, "resize_modes"), ["interactive"] * NCOLS, "boot modes")   # 4
        assert_eq(_painted_widths(tf), [w - GUTTER for w in BOOT_W],
                  "both agree with the strip, so neither is wrong yet")         # 5

        # ── (B) the two plurals part ──────────────────────────────────
        # The measurement this round entered on.
        tf.invoke("/external/set_all_resize_modes", "stretch")
        wait_until(lambda: _h(tf, "resize_modes")[0] == "stretch",
                   desc="the row stretches")                                    # 6
        assert_eq(_h(tf, "sizes"), BOOT_W,
                  "the stored sizes survive the mode switch — that is their job")  # 7
        shares = _h(tf, "section_sizes")
        assert shares != BOOT_W, f"the effective plural has parted from it: {shares}"  # 8
        assert_eq(sum(shares), AVAILABLE_W,
                  "the shares divide the whole published viewport")             # 9
        assert_eq(_painted_widths(tf), [s - GUTTER for s in shares],
                  "and it is the effective plural the rects agree with")        # 10
        assert _painted_widths(tf) != [w - GUTTER for w in BOOT_W], \
            "which the stored plural, read alone, would have got wrong"         # 11

        # ── (C) singular and plural cannot disagree ───────────────────
        for logical in range(NCOLS):
            assert_eq(_h(tf, f"section_size.{logical}"), shares[logical],
                      f"section_size.{logical} reads the plural")               # 12-16
        assert_eq(_h(tf, "visible_widths"), shares,
                  "and the visual-order projection is the same numbers")        # 12

        # ── (D) the default governs, and spares the hidden ────────────
        _reset(tf)
        tf.invoke("/external/set_section_hidden", "2:true")
        wait_until(lambda: _h(tf, "hidden")[2], desc="Size is hidden")
        tf.intervene("/external/default_section_size", 70)
        wait_until(lambda: _h(tf, "default_section_size") == 70,
                   desc="the new default is in force")                          # 13
        assert_eq(_h(tf, "sizes"), [70, 70, BOOT_W[2], 70, 70],
                  "every SHOWN section took it; the hidden one kept its size")  # 14
        assert_eq(_h(tf, "hidden_count"), 1, "and it is still hidden")          # 15

        # ── (E) reset without naming the constant ─────────────────────
        # The answer is the row, not the number written: under a stretch header
        # those are different, which is the whole point of the round.
        tf.invoke("/external/set_all_resize_modes", "stretch")
        wait_until(lambda: _h(tf, "resize_modes")[0] == "stretch", desc="stretch again")
        produced = tf.invoke("/external/reset_default_section_size", None)
        assert_eq(_h(tf, "default_section_size"), DEFAULT_SIZE,
                  "the reset reached the constant without the caller naming it")  # 16
        assert_eq(produced, _h(tf, "section_sizes"),
                  "and it answered the row it produced")                        # 17
        assert produced != _h(tf, "sizes"), \
            f"which is NOT the number it wrote: {produced} vs {_h(tf, 'sizes')}"  # 18
        assert_eq(_h(tf, "sizes"), [DEFAULT_SIZE, DEFAULT_SIZE, BOOT_W[2],
                                    DEFAULT_SIZE, DEFAULT_SIZE],
                  "the stored sizes did take the default, hidden section aside")  # 19

        # ── (E2) the default cannot name a size the header refuses ────
        # The three scalars are one rule between them, so the default is
        # clamped where it is READ rather than where it is written: move a
        # bound afterwards and the default follows, with no second write path
        # that could have been forgotten.
        _reset(tf)
        tf.intervene("/external/default_section_size", 300)
        wait_until(lambda: _h(tf, "default_section_size") == 300,
                   desc="a wide default, within the unbounded ceiling")         # 20
        tf.intervene("/external/max_section_size", 120)
        wait_until(lambda: _h(tf, "default_section_size") == 120,
                   desc="the ceiling moved AFTER, and took the default with it")  # 21
        tf.intervene("/external/min_section_size", 200)
        assert_eq(_h(tf, "default_section_size"), 200,
                  "and the floor does the same from the other side")            # 22
        # Not lost, only clamped: widen the bounds and the asked-for value is
        # back, which is why it is stored raw.
        tf.intervene("/external/min_section_size", FLOOR)
        tf.intervene("/external/max_section_size", UNBOUNDED)
        wait_until(lambda: _h(tf, "default_section_size") == 300,
                   desc="the value asked for was kept, not overwritten")        # 23

        # ── (F) the rules travel with the outcomes ──────────────────── `ColumnLayoutState`
        # calls itself the peer of the toolkit's `saveState()`, which carries these
        # three.
        _reset(tf)
        tf.intervene("/external/min_section_size", 60)
        tf.intervene("/external/max_section_size", 150)
        tf.intervene("/external/default_section_size", 80)
        wait_until(lambda: _h(tf, "default_section_size") == 80, desc="rules set")
        saved = _h(tf, "state")
        assert_eq(saved["min_section_size"], 60, "the floor is in the snapshot")   # 24
        assert_eq(saved["max_section_size"], 150, "and the ceiling")               # 25
        assert_eq(saved["default_section_size"], 80, "and the default")            # 26

        _reset(tf)
        assert_eq(_h(tf, "max_section_size"), UNBOUNDED, "a header with no ceiling")  # 27
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "max_section_size") == 150,
                   desc="the restore brought the rule, not only the result")     # 28
        assert_eq(_h(tf, "state"), saved, "the whole snapshot came back")         # 29
        assert_eq(tf.invoke("/external/resize_section", "0:9999"), 150,
                  "so the restored header refuses what the saved one refused")    # 30

        # ── (F2) a blob saved by an older build still restores ────────
        # A client's stored layout predates these fields. Absent means "taken
        # before this round" — the rule `modes` and `sort_indicator` already
        # use — and decodes to the constants. Zero bounds would be a header
        # whose sections must be at most zero wide.
        older = {k: v for k, v in saved.items()
                 if k not in ("default_section_size", "min_section_size",
                              "max_section_size")}
        assert "min_section_size" not in older, "the older shape really lacks them"
        tf.intervene("/external/state", older)
        wait_until(lambda: _h(tf, "min_section_size") == FLOOR,
                   desc="the floor came back as the constant, not as 0")
        assert_eq(_h(tf, "max_section_size"), UNBOUNDED,
                  "and the ceiling, which zero would have made unusable")
        assert_eq(_h(tf, "default_section_size"), DEFAULT_SIZE, "and the default")
        assert_eq(_h(tf, "sizes"), saved["sizes"],
                  "while the fields the older blob DID carry landed unchanged")
        # Back to the bounded header the next block reasons about.
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "max_section_size") == 150,
                   desc="the current-shape blob restored again")

        # ── (G) a restore is not order-dependent ──────────────────────
        crossed = dict(saved, min_section_size=200, max_section_size=100)
        assert_out_of_range(
            lambda: tf.intervene("/external/state", crossed),
            saying="the saved bounds cross",
        )                                                                        # 31
        assert_eq(_h(tf, "max_section_size"), 150, "and nothing at all was written")  # 32

        # A wide layout into a narrow header: the INCOMING ceiling governs, so
        # the widths survive. Writing them before the bounds would truncate each
        # one on the way in, and no later bound could widen it back.
        wide = dict(saved, sizes=[400] * NCOLS, max_section_size=UNBOUNDED,
                    default_section_size=DEFAULT_SIZE)
        tf.intervene("/external/state", wide)
        wait_until(lambda: _h(tf, "sizes") == [400] * NCOLS,
                   desc="the wide layout landed intact")                         # 33
        assert_eq(_h(tf, "max_section_size"), UNBOUNDED,
                  "because the incoming ceiling arrived first")                  # 34

        # ── (H) the painted rule ──────────────────────────────────────
        _reset(tf)
        readout = _readout(tf)
        assert readout.startswith(f"sizes Name={BOOT_W[0]}"), \
            f"interactive: the readout leads with the size on screen: {readout}"  # 35
        assert "| stored " not in readout, \
            f"and with nothing to tell apart, it says it once: {readout}"        # 36
        assert f"| default {DEFAULT_SIZE} |" in readout, \
            f"the default is named: {readout}"                                   # 37

        tf.invoke("/external/set_all_resize_modes", "stretch")
        wait_until(lambda: "| stored " in _readout(tf),
                   desc="the readout tells the two apart once they differ")      # 38
        readout = _readout(tf)
        share0 = _h(tf, "section_sizes")[0]
        assert readout.startswith(f"sizes Name={share0}"), \
            f"leading with the painted size, not the stored one: {readout}"      # 39
        assert f"| stored Name={BOOT_W[0]}" in readout, \
            f"and still naming what a restore would replay: {readout}"           # 40
        assert_eq(_painted_widths(tf)[0], share0 - GUTTER,
                  "the readout's first number is the rect above it")             # 41


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1493 §5.27 §2#7 — QHeaderView defaultSectionSize + the effective plural",
        body,
    ))
