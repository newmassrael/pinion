#!/usr/bin/env python3
"""R1498 §5.27 §2#7 §2#2 — the row fills the viewport it was given.

the toolkit reference: `stretchLastSection` — "whether the last visible
section in the header takes up all the available space", default `false`, and
"if this value is set to true, this property will override the resize mode set
on the last section in the header". `write()` serialises it,
so `saveState()` carries it.

Measured on this very binding before the round, over the real wire:

    query  stretch_last_section  -> UnknownIntrospectPath
    query  visible_total         -> 570
    query  available_width       -> 640

Seventy pixels of the strip were painted by nothing, and there was no rule
saying whether they should be — so no way to ask for the other answer.

`Stretch` on the last column is NOT that rule, which the same session measured:

    set_resize_mode 4:stretch    -> section_sizes [150, 90, 100, 130, 170]
    set_section_hidden 4:true    -> visible_total 470 of a 640-wide viewport
    move_section 4:0             -> the 170-wide fill paints at x=0

A mode belongs to a column and travels with it. This rule belongs to the header
and stays at the end of the row, which is what makes it a different thing and
not a spelling of one that already existed.

The rule is implemented as exactly that override: the last painted section
becomes a `Stretch` section, and the division that already exists gives it what
the others leave over. There is no second sizing algorithm — a header with one
stretching section and this rule on has two of them, sharing as they always did.

Nothing is written to any section. The toolkit has to remember a `lastSectionSize`
because the toolkit writes the stretched width into the section; this module already
keeps the stored size and the painted size apart (R1493), and that split is what
makes the memory unnecessary.

What this asserts:

  (A) THE RULE IS READABLE AND OFF — the toolkit's default, and the measurement above
      replayed: the row falls short of the strip it was told to fill.
  (B) THE LAST SECTION FILLS — one flag, and the leftover has an owner.
  (C) KEYED BY POSITION, NOT BY COLUMN — hiding the filled section promotes the
      next one; dragging it to the front leaves the fill at the end. The
      `Stretch`-mode contrast is measured in the same run, on the same header.
  (D) IT OVERRIDES THE MODE SET ON THE LAST SECTION — the toolkit's own words, and the
      set mode is still reported beside the applied one.
  (E) IT SHARES — with the other `Stretch` sections, through the one division.
  (F) A FILLED SECTION DOES NOT PAY — R1494's cascade skips it, because
      squeezing a derived width moves no pixels while counting the debt paid.
  (G) NOTHING STORED MOVED — so withdrawing the rule is its own undo, with no
      remembered size to restore.
  (H) THE PAINTED RULE — the readout names it, names the mode the header
      APPLIES, and the saveState peer carries it.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1498_stretch_last_section.py
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

FLOOR = 40
RESIZE_STEP = 20
UNBOUNDED = 2**32 - 1
DEFAULT_SIZE = 100
BOOT_TOTAL = sum(BOOT_W)
LEFTOVER = AVAILABLE_W - BOOT_TOTAL


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _readout(tf) -> str:
    return find_by_tag(_paint(tf), LAYOUT_TAG)["content"]


def _reset(tf, *, stretch_last: bool = False) -> None:
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
            "cascading_section_resizes": False,
            "stretch_last_section": stretch_last,
        },
    )
    wait_until(
        lambda: _h(tf, "sizes") == BOOT_W
        and _h(tf, "stretch_last_section") == stretch_last,
        desc=f"layout reset (stretch_last={stretch_last})",
    )


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) the rule is readable, and off ─────────────────────────
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                   desc="the strip paints")
        assert_eq(_h(tf, "stretch_last_section"), False,
                  "off by default, as in the toolkit")                                    # 1
        assert_eq(_h(tf, "available_width"), AVAILABLE_W, "the strip's width")   # 2
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL,
                  "and the row is narrower than it")                             # 3
        assert_eq(AVAILABLE_W - BOOT_TOTAL, LEFTOVER,
                  "the pixels nothing was painting")                             # 4
        assert "| stretch-last off |" in _readout(tf), \
            f"the readout names the rule: {_readout(tf)}"                        # 5

        # ── (B) the last section fills ────────────────────────────────
        tf.intervene("/external/stretch_last_section", True)
        wait_until(lambda: _h(tf, "stretch_last_section"), desc="the rule is on")
        assert_eq(_h(tf, "section_sizes"), [150, 90, 100, 130, 100 + LEFTOVER],
                  "the last section took exactly what was left over")            # 6
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W, "so the row fills")      # 7
        assert_eq(_h(tf, "sizes"), BOOT_W,
                  "and not one stored width moved to do it")                     # 8
        assert_eq(_h(tf, "section_position.4"), 470,
                  "the geometry a hit test reads agrees")                        # 9

        # ── (C) keyed by position, not by column ──────────────────────
        tf.invoke("/external/set_section_hidden", "4:true")
        wait_until(lambda: _h(tf, "hidden")[4], desc="Owner hidden")
        assert_eq(_h(tf, "visible_sections"), [0, 1, 2, 3])                      # 10
        assert_eq(_h(tf, "section_sizes"), [150, 90, 100, 300, 100],
                  "hiding the filled section promotes the one now last")         # 11
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W, "the row still fills")   # 12

        _reset(tf, stretch_last=True)
        tf.invoke("/external/move_section", "4:0")
        wait_until(lambda: _h(tf, "order")[0] == 4, desc="Owner dragged to the front")
        assert_eq(_h(tf, "section_sizes"), [150, 90, 100, 130 + LEFTOVER, 100],
                  "dragged to the front it is an ordinary section again")        # 13
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "and the fill stayed at the end of the row")                   # 14

        # The contrast, on the same header: a MODE is keyed to the column.
        _reset(tf)
        tf.invoke("/external/set_resize_mode", "4:stretch")
        wait_until(lambda: _h(tf, "resize_mode.4") == "stretch", desc="Owner stretches")
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "a stretching last column fills the row too")                  # 15
        tf.invoke("/external/set_section_hidden", "4:true")
        wait_until(lambda: _h(tf, "hidden")[4], desc="Owner hidden")
        assert_eq(_h(tf, "visible_total"), 470,
                  "but hiding it drops the fill, where the rule promoted a "
                  "successor")                                                   # 16
        tf.invoke("/external/set_section_hidden", "4:false")
        tf.invoke("/external/move_section", "4:0")
        wait_until(lambda: _h(tf, "order")[0] == 4, desc="Owner to the front")
        assert_eq(_h(tf, "placements")[0], {"visual": 0, "logical": 4, "x": 0,
                                            "size": 100 + LEFTOVER},
                  "and dragging it paints the fill at the FRONT, where the "
                  "rule left it at the end")                                     # 17

        # ── (D) it overrides the mode set on the last section ─────────
        _reset(tf, stretch_last=True)
        tf.invoke("/external/set_resize_mode", "4:fixed")
        wait_until(lambda: _h(tf, "resize_mode.4") == "fixed", desc="Owner is fixed")
        assert_eq(_h(tf, "section_sizes")[4], 100 + LEFTOVER,
                  "a Fixed last section is filled anyway — the toolkit's own words")      # 18
        assert_eq(_h(tf, "effective_resize_mode.4"), "stretch",
                  "the mode the header APPLIES")                                 # 19
        assert_eq(_h(tf, "resize_mode.4"), "fixed",
                  "beside the one that was SET, which is unchanged")             # 20
        assert_eq(_h(tf, "effective_resize_modes"),
                  ["interactive"] * 4 + ["stretch"],
                  "and the plural says what the singular says (the R1493 rule)")  # 21
        assert_eq(_h(tf, "resize_modes"), ["interactive"] * 4 + ["fixed"],
                  "as does the stored plural, for the stored mode")              # 22

        # ── (E) it shares ─────────────────────────────────────────────
        _reset(tf, stretch_last=True)
        tf.invoke("/external/set_resize_mode", "1:stretch")
        wait_until(lambda: _h(tf, "resize_mode.1") == "stretch", desc="Type stretches")
        assert_eq(_h(tf, "section_sizes"), [150, 130, 100, 130, 130],
                  "640 less the 380 the other three take, split two ways")       # 23
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "one division, not a second algorithm bolted beside it")       # 24

        # ── (F) a filled section does not pay ─────────────────────────
        _reset(tf, stretch_last=True)
        tf.intervene("/external/cascading_section_resizes", True)
        wait_until(lambda: _h(tf, "cascading_section_resizes"), desc="cascading on")
        assert_eq(tf.invoke("/external/interactive_resize_section", "0:400"), 400,
                  "grow the first section by 250")                               # 25
        assert_eq(_h(tf, "sizes"), [400, FLOOR, FLOOR, FLOOR, 100],
                  "the three interactive followers paid to the floor; the "
                  "filled one was skipped and kept the width it comes back at")  # 26
        assert_eq(_h(tf, "section_sizes"), [400, FLOOR, FLOOR, FLOOR, 120],
                  "and the fill absorbed what they could not cover")             # 27
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "so the row is still exactly as wide as the strip")            # 28

        # ── (G) nothing stored moved ──────────────────────────────────
        _reset(tf, stretch_last=True)
        assert_eq(_h(tf, "section_sizes")[4], 100 + LEFTOVER, "filled")          # 29
        tf.intervene("/external/stretch_last_section", False)
        wait_until(lambda: _h(tf, "stretch_last_section") is False,
                   desc="the rule is withdrawn")
        assert_eq(_h(tf, "section_sizes"), BOOT_W,
                  "withdrawing it hands the section back its own width, with "
                  "no remembered size to restore")                               # 30
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL)                           # 31

        # ── (H) the painted rule ──────────────────────────────────────
        _reset(tf)
        assert "| modes iiiii |" in _readout(tf), _readout(tf)                   # 32
        assert "stored modes" not in _readout(tf), \
            f"nothing is overridden, so there is no second row: {_readout(tf)}"  # 33
        # The `f` key is this binding's gesture for the rule — a header-wide
        # rule, so unlike `m` and `h` it needs no cursor. It does need the
        # strip focused: `apply_key` refuses a key aimed at anything else.
        tf.request("focus/set", {"tag": HDR})
        assert_eq(tf.request("focus/get").result.get("focused"), HDR,
                  "the strip has the keyboard")                                  # 34
        tf.key(path=f"{HDR}#0", name="f")
        wait_until(lambda: "| stretch-last on |" in _readout(tf),
                   desc="the f key turns it on and the readout names it")
        assert "| modes iiiis | stored modes iiiii |" in _readout(tf), \
            f"the readout names the APPLIED mode first: {_readout(tf)}"          # 35
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W, "and the row fills")     # 36
        tf.key(path=f"{HDR}#0", name="f")
        wait_until(lambda: "| stretch-last off |" in _readout(tf),
                   desc="and the same key turns it back off")
        assert_rpc_error(
            lambda: tf.intervene("/external/stretch_last_section", 1),
            data="InterveneTypeMismatch",
        )                                                                        # 37

        # The saveState peer carries it, so a restore replays the rule.
        tf.intervene("/external/stretch_last_section", True)
        wait_until(lambda: _h(tf, "stretch_last_section"), desc="on again")
        saved = _h(tf, "state")
        assert_eq(saved["stretch_last_section"], True,
                  "the snapshot carries the toolkit's property")                          # 38
        assert_eq(saved["sizes"], BOOT_W,
                  "and the widths it carries are the stored ones")               # 39
        _reset(tf)
        assert_eq(_h(tf, "stretch_last_section"), False, "a header without it")  # 40
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "stretch_last_section"),
                   desc="the restore brought the rule back")
        assert_eq(_h(tf, "section_sizes")[4], 100 + LEFTOVER,
                  "and the restored header fills from the widths it was given")  # 41
        assert_eq(_h(tf, "state"), saved, "the whole snapshot came back")        # 42

        # A snapshot taken before this round carries no such field. Here the
        # toolkit's default and the older header AGREE — that header did not
        # fill either — so unlike the R1496 permissions there is no divergence
        # to encode, and an older layout restores into a header that leaves its
        # strip alone. (A counterfactual put `true` here and every assertion above
        # still passed: nothing was driving the absent-field path over the
        # wire.)
        older = {k: v for k, v in saved.items() if k != "stretch_last_section"}
        assert "stretch_last_section" not in older, "the pre-R1498 shape"        # 43
        tf.intervene("/external/state", older)
        wait_until(lambda: _h(tf, "stretch_last_section") is False,
                   desc="an older snapshot restores a header that does not fill")
        assert_eq(_h(tf, "section_sizes"), BOOT_W,
                  "so the row it paints is the one that snapshot described")     # 44
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL)                           # 45

        # ── (I) the keystroke obeys the rule the readout names ────────
        # R1494's split, over the real wire: `[` / `]` is this binding's
        # interactive resize, so it is gated on the mode the header APPLIES or
        # it accepts the keystroke and paints nothing. (A counterfactual
        # collapsed that read to the stored mode and every assertion above
        # still passed — the gate is the binding's, and nothing here was
        # pressing the key.)
        _reset(tf, stretch_last=True)
        tf.key(path=f"{HDR}#0", name="End")
        wait_until(lambda: _h(tf, "focused_index") == 4,
                   desc="cursor onto the last section")
        tf.key(path=f"{HDR}#0", name="]")
        assert_eq(_h(tf, "sizes"), BOOT_W,
                  "the key is refused on the section the rule is filling")       # 46
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "and nothing moved behind the refusal")                        # 47
        tf.key(path=f"{HDR}#0", name="ArrowLeft")
        wait_until(lambda: _h(tf, "focused_index") == 3,
                   desc="cursor onto its neighbour")
        tf.key(path=f"{HDR}#0", name="]")
        wait_until(lambda: _h(tf, "sizes")[3] == BOOT_W[3] + RESIZE_STEP,
                   desc="which the user may still size")
        assert_eq(_h(tf, "section_sizes")[4], 100 + LEFTOVER - RESIZE_STEP,
                  "and the fill gave up exactly what its neighbour took")        # 48
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "so the row still fills the strip")                            # 49


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1498 §5.27 §2#7 — header view stretchLastSection: the row fills its "
        "viewport",
        body,
    ))
