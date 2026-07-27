#!/usr/bin/env python3
"""R1452 §5.27 §5.40 §2#7 §2#2 — a column fills the row, or fits its content.

Qt reference: `QHeaderView::setSectionResizeMode` — `Interactive`, `Fixed`,
`Stretch`, `ResizeToContents`. Before this, every pinion grid had exactly ONE
sizing policy: a stored number. A column could not fill the viewport and could
not fit its content, because there was nowhere to say where its size comes from.

R1451 gave the header its state (order x size x hidden, keyed by logical
section); R1452 makes the size DERIVABLE, and because R1451 resolved geometry in
a single walk, every downstream answer — positions, the hit test, the strip
total, the a11y tree — follows the new sizes with no second rule.

Where Qt cannot follow: `setSectionResizeMode` is a C++ call and the resulting
widths are only observable by painting. Here the policy is typed data
(`resize_modes`), the two inputs the derived modes read are typed data
(`content_widths`, `available_width`), and both are readable AND writable over
the wire.

What this asserts:

  (A) BOOT — the view fn's published inputs reach the SAME layout the external
      mutates. `available_width` reading back is the whole shared-instance
      wiring in one assertion; `content_widths` are the MEASURED monospace
      hints, not constants in the source.
  (B) STRETCH — the stretch sections take what the others leave over (not an
      equal split of the whole), the row totals EXACTLY the published width,
      and the painted rects agree with the model.
  (C) RESIZE_TO_CONTENTS — the section sizes to its content hint, and the
      painted TEXT actually fits inside it. An invented hint would pass every
      model assertion and still clip the text, so this is the one that matters.
  (D) FIXED — the mode where the two questions differ: the user gesture is
      refused, the programmatic call is not.
  (E) MODE TRAVELS WITH ITS SECTION — the policy is keyed by logical section
      like the size it replaces, so dragging the column takes it along.
  (F) STATE — the modes round-trip, and a pre-R1452 snapshot (no `modes` field
      at all) still restores.
  (G) TYPED REFUSALS — a misspelled mode is rejected rather than silently
      defaulted, and nothing changes.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1452_section_resize_modes.py
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
# The binding's constants: window 700 less 2x30 margin, and the cell padding a
# content-fitted column adds to its text on each side.
AVAILABLE_W = 640
CELL_PAD = 12
GUTTER = 2
STEP = 20
# The longest string each column shows, in characters (header label or cell).
WIDEST_CHARS = [10, 5, 6, 10, 5]


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    return None if node is None else node["rect"]


def _placements(tf):
    return _h(tf, "placements")


def _visual_of(tf, logical: int) -> int:
    return _h(tf, f"visual_index.{logical}")


def _reset(tf) -> None:
    """Back to the boot layout through the restore path itself."""
    tf.intervene("/external/state", {
        "order": list(range(NCOLS)),
        "sizes": BOOT_W,
        "hidden": [False] * NCOLS,
        "modes": ["interactive"] * NCOLS,
    })
    wait_until(lambda: _h(tf, "resize_modes") == ["interactive"] * NCOLS,
               desc="layout reset")


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        # ── (A) the view's publish reaches the external's layout ──────
        wait_until(lambda: _rect(tf, f"{HDR}#0") is not None, desc="the strip paints")
        # If the view fn and the external had resolved two different cached
        # layouts, this would still read Null — the whole shared-instance
        # wiring, in one assertion.
        wait_until(lambda: _h(tf, "available_width") == AVAILABLE_W,
                   desc="the view published its viewport")                          # 1
        assert_eq(_h(tf, "resize_modes"), ["interactive"] * NCOLS,
                  "boot: Qt's default policy, everywhere")                          # 2

        hints = _h(tf, "content_widths")
        assert_eq(len(hints), NCOLS, "one content hint per section")                # 3
        # The hints are MEASURED: a single monospace cell, times the longest
        # string, plus the padding. Recovering the same cell width from every
        # column is what shows the number came from a measurement rather than
        # from a constant per column.
        cells = [(h - 2 * CELL_PAD) // c for h, c in zip(hints, WIDEST_CHARS)]
        assert_eq(len(set(cells)), 1,
                  f"every hint resolves to one measured cell width: {cells}")       # 4
        cell_w = cells[0]
        assert cell_w > 0, f"the measured cell has a real width ({cell_w})"         # 5
        assert_eq(hints[0], 2 * CELL_PAD + WIDEST_CHARS[0] * cell_w,
                  "Name's hint fits its longest cell, report.pdf")                  # 6
        assert hints[1] < hints[3], "Type needs less room than Modified"            # 7
        assert "modes iiiii" in find_by_tag(_paint(tf), LAYOUT_TAG)["content"], \
            "the policy is scene-as-data too"                                        # 8

        # ── (B) stretch takes the LEFTOVER, and the row fills exactly ──
        tf.invoke("/external/set_resize_mode", "1:stretch")
        widths = tf.invoke("/external/set_resize_mode", "4:stretch")
        # 640 less Name(150) + Size(100) + Modified(130) = 260, split two ways.
        # An equal split of the whole 640 would answer 128 — this discriminates.
        assert_eq(widths, [150, 130, 100, 130, 130],
                  "the stretch sections divide what the others leave over")         # 9
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W,
                  "and the row fills exactly the width it was given")               # 10
        assert_eq(_rect(tf, f"{HDR}#1")["w"], 130 - GUTTER,
                  "the painted section is the stretched width")                     # 11
        assert_eq(_rect(tf, f"{HDR}#4")["w"], 130 - GUTTER, "both of them")         # 12
        # Hiding one hands its share to the other, because the division is over
        # the PAINTED sections.
        tf.invoke("/external/set_section_hidden", "1:true")
        wait_until(lambda: _h(tf, "hidden_count") == 1, desc="Type hides")
        assert_eq(_h(tf, "visible_widths"), [150, 100, 130, 260],
                  "the remaining stretch section takes the whole leftover")         # 13
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W, "the row still fills")      # 14
        tf.invoke("/external/set_section_hidden", "1:false")
        wait_until(lambda: _h(tf, "hidden_count") == 0, desc="Type returns")

        # ── (C) contents: the size is real, and the text FITS ─────────
        _reset(tf)
        tf.invoke("/external/set_all_resize_modes", "resize_to_contents")
        wait_until(lambda: _h(tf, "visible_widths") == hints,
                   desc="every section sized to its content")                       # 15
        v0 = _visual_of(tf, 0)
        section = _rect(tf, f"{HDR}#{v0}")
        assert_eq(section["w"], hints[0] - GUTTER, "Name paints at its hint")       # 16
        # The claim an invented hint would fail: the widest cell's laid-out TEXT
        # fits inside the section, with the padding it was given on both sides.
        widest = _rect(tf, f"colbody#0_{v0}")  # row 0 is "report.pdf", 10 chars
        # The measured cell is a WHOLE number of pixels, so it is the ceiling of
        # the face's real per-character advance and the hint is an upper bound —
        # the right direction for a size hint. Pin the band it must stay in: an
        # invented cell width lands outside it, and so does a hint that stopped
        # tracking the face.
        n = WIDEST_CHARS[0]
        assert n * (cell_w - 1) < widest["w"] <= n * cell_w, (
            f"the painted text ({widest['w']}) sits inside the measured band "
            f"({n * (cell_w - 1)}, {n * cell_w}]"
        )                                                                            # 17
        assert_eq(widest["x"], section["x"] + CELL_PAD, "inset by the padding")     # 18
        assert widest["x"] + widest["w"] + CELL_PAD <= section["x"] + hints[0], \
            "and it FITS, padding and all, inside its own column"                    # 19
        assert_eq(_h(tf, "content_width.3"), hints[3], "per-section read agrees")   # 20

        # ── (D) Fixed: the two questions, and their different answers ──
        _reset(tf)
        tf.request("focus/set", {"tag": HDR})
        assert_eq(tf.request("focus/get").result.get("focused"), HDR, "strip focused")  # 21
        tf.key(path=f"{HDR}#0", name="ArrowRight")
        wait_until(lambda: _h(tf, "focused_index") == 0, desc="cursor on Name")
        tf.key(path=f"{HDR}#0", name="]")
        wait_until(lambda: _h(tf, "sizes")[0] == BOOT_W[0] + STEP,
                   desc="Interactive: the user may size it")                        # 22
        tf.key(path=f"{HDR}#0", name="m")
        wait_until(lambda: _h(tf, "resize_mode.0") == "fixed", desc="m cycles to Fixed")  # 23
        tf.key(path=f"{HDR}#0", name="]")
        assert_eq(_h(tf, "sizes")[0], BOOT_W[0] + STEP,
                  "Fixed: the user gesture is refused")                             # 24
        assert_eq(tf.invoke("/external/resize_section", "0:300"), 300,
                  "but the programmatic call is not — that is what Fixed means")    # 25
        # The rest of the cycle, and back.
        tf.key(path=f"{HDR}#0", name="m")
        wait_until(lambda: _h(tf, "resize_mode.0") == "stretch", desc="-> Stretch")  # 26
        tf.key(path=f"{HDR}#0", name="m")
        wait_until(lambda: _h(tf, "resize_mode.0") == "resize_to_contents",
                   desc="-> ResizeToContents")                                      # 27
        tf.key(path=f"{HDR}#0", name="m")
        wait_until(lambda: _h(tf, "resize_mode.0") == "interactive", desc="-> back")  # 28
        assert_eq(_h(tf, "sizes")[0], 300,
                  "and the size the derived modes ignored was kept, not discarded")  # 29

        # ── (E) the policy travels with its section ───────────────────
        _reset(tf)
        tf.invoke("/external/set_resize_mode", "0:stretch")
        assert_eq(_h(tf, "visible_widths"), [220, 90, 100, 130, 100],
                  "Name stretches into the leftover")                               # 30
        tf.invoke("/external/move_section", "0:4")
        wait_until(lambda: _h(tf, "order") == [1, 2, 3, 4, 0], desc="Name to the end")
        assert_eq(_h(tf, "resize_mode.0"), "stretch", "the mode moved with it")     # 31
        assert_eq(_h(tf, "visible_widths"), [90, 100, 130, 100, 220],
                  "and it is still the section taking the leftover")                # 32
        assert_eq(_h(tf, "section_position.0"), 420, "at its new place")            # 33
        assert_eq(_h(tf, "visible_total"), AVAILABLE_W, "the row still fills")      # 34

        # ── (F) the modes round-trip, and an older snapshot restores ───
        saved = _h(tf, "state")
        assert_eq(saved["modes"], ["stretch"] + ["interactive"] * 4,
                  "saveState carries the policy, as Qt's does")                     # 35
        tf.invoke("/external/set_all_resize_modes", "fixed")
        wait_until(lambda: _h(tf, "resize_modes") == ["fixed"] * NCOLS, desc="drift")
        tf.intervene("/external/state", saved)
        wait_until(lambda: _h(tf, "state") == saved, desc="restoreState")
        assert_eq(_h(tf, "resize_modes"), ["stretch"] + ["interactive"] * 4,
                  "the policy came back with the rest of the layout")               # 36
        # A pre-R1452 snapshot has no `modes` key at all and still restores.
        tf.intervene("/external/state", {
            "order": [4, 3, 2, 1, 0], "sizes": BOOT_W, "hidden": [False] * NCOLS,
        })
        wait_until(lambda: _h(tf, "order") == [4, 3, 2, 1, 0], desc="older shape")  # 37
        assert_eq(_h(tf, "resize_modes"), ["interactive"] * NCOLS,
                  "and reads as the default policy")                                # 38

        # ── (G) refusals are typed, and change nothing ────────────────
        before = _h(tf, "state")
        assert_rpc_error(lambda: tf.invoke("/external/set_resize_mode", "0:Stretch"))  # 39
        assert_rpc_error(lambda: tf.invoke("/external/set_resize_mode", "9:fixed"))    # 40
        assert_rpc_error(lambda: tf.invoke("/external/set_all_resize_modes", "snug"))  # 41
        assert_rpc_error(lambda: tf.intervene("/external/state", {
            "order": [0, 1, 2, 3, 4], "sizes": BOOT_W, "hidden": [False] * NCOLS,
            "modes": ["interactive", "Stretch", "fixed", "fixed", "fixed"],
        }), data="InterveneTypeMismatch")                                           # 42
        assert_rpc_error(lambda: tf.intervene("/external/content_widths", [1, 2]),
                         data="OutOfRange")                                         # 43
        assert_eq(_h(tf, "state"), before, "five refusals, nothing moved")          # 44


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1452 §5.27 §2#7 §2#2 — QHeaderView section resize modes",
        body,
    ))
