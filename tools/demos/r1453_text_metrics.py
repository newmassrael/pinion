#!/usr/bin/env python3
"""R1453 §5.36 §5.27 §2#7 — a view function can ask how wide a string will be.

Qt reference: `QFontMetrics::horizontalAdvance` / `boundingRect(text)`. Qt's
item views lean on it — `QAbstractItemDelegate::sizeHint` measures the cell's
text, which is how `QHeaderView::ResizeToContents` knows what "the content's
size" is.

pinion could measure exactly ONE thing from a view fn: the cell of a MONOSPACE
face at a size (`MonospaceMetrics`, R1003). A proportional face has no cell, so
"how wide is `report.pdf` in the grid's font" had no answer at all. R1452 worked
around it by forcing the grid into a monospace face and multiplying a character
count by the cell — which is an UPPER BOUND (`CellMetric` is whole pixels, so it
is the ceiling of the real advance), and only for text you were willing to make
monospace. R1453 retires both limits.

What this asserts:

  (A) EXACT, NOT A BOUND — a content-fitted column is its widest PAINTED string
      plus the padding, TO THE PIXEL, for every column. This is R1452's band
      assertion collapsed to an equality: the debt that round recorded, paid.
      It is also the discriminator — a constant, or a character count times one
      cell, does not land exactly on five different measured widths.
  (B) EVERY CELL FITS — for every column and every row, the painted text plus
      padding is inside its own column. An invented hint passes every model
      assertion and still clips text, so this is the one that matters.
  (C) THE HINT DESCRIBES THE CONTENT, NOT THE LAYOUT — sections widen and narrow
      through the wire and the hints stay put; squeezed to the floor, a cell
      still reports the width it NEEDS, and switching back to contents gives it
      exactly that again.

Every assertion is **self-calibrating**: the expected value is derived from the
same run, never from a font-specific constant. The rendering face is the host's
system font (pinion bundles none), so an assertion about which strings a face
makes wider would be a host-dependent flake — this file makes none.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1453_text_metrics.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
NCOLS = 5
NROWS = 6
CELL_PAD = 12


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    return None if node is None else node["rect"]


def _visual_of(tf, logical: int) -> int:
    return _h(tf, f"visual_index.{logical}")


def _painted_cells(tf, logical: int) -> list[tuple[str, int]]:
    """(text, painted width) for every body cell of a logical column."""
    v = _visual_of(tf, logical)
    out = []
    snap = _paint(tf)
    for r in range(NROWS):
        node = find_by_tag(snap, f"colbody#{r}_{v}")
        assert node is not None, f"cell {r} of column {logical} paints"
        out.append((node.get("content") or "", node["rect"]["w"]))
    return out


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        wait_until(lambda: _rect(tf, f"{HDR}#0") is not None, desc="the strip paints")
        wait_until(lambda: _h(tf, "content_widths")[0] > 0,
                   desc="the view published measured hints")                        # 1
        hints = _h(tf, "content_widths")
        assert_eq(len(hints), NCOLS, "one hint per section")                        # 2

        # ── (A) exact, not a bound ────────────────────────────────────
        tf.invoke("/external/set_all_resize_modes", "resize_to_contents")
        wait_until(lambda: _h(tf, "visible_widths") == hints,
                   desc="every column fits its content")                            # 3
        for logical in range(NCOLS):
            cells = _painted_cells(tf, logical)
            widest_px = max(w for _, w in cells)
            header = _rect(tf, f"colhdr_label#{_visual_of(tf, logical)}")["w"]
            assert_eq(
                max(widest_px, header) + 2 * CELL_PAD,
                hints[logical],
                f"column {logical}'s hint is exactly its widest painted string",
            )                                                                        # 4-8
        # The hints are not one number: each column measured its own content.
        assert len(set(hints)) > 1, f"the columns measured differently: {hints}"    # 9

        # ── (B) every cell fits inside its own column ─────────────────
        for logical in range(NCOLS):
            v = _visual_of(tf, logical)
            section = _rect(tf, f"{HDR}#{v}")
            for r in range(NROWS):
                cell = _rect(tf, f"colbody#{r}_{v}")
                assert cell["x"] + cell["w"] + CELL_PAD <= section["x"] + hints[logical], (
                    f"row {r} of column {logical} fits, padding and all"
                )                                                                    # 10-39

        # ── (C) the hints describe the content, not the layout ────────
        before = _h(tf, "content_widths")
        tf.invoke("/external/set_all_resize_modes", "interactive")
        tf.invoke("/external/resize_section", "0:300")
        wait_until(lambda: _h(tf, "sizes")[0] == 300, desc="Name widens")
        assert_eq(_h(tf, "content_widths"), before,
                  "resizing a column does not change what its content needs")       # 40
        tf.invoke("/external/resize_section", "0:40")
        wait_until(lambda: _h(tf, "sizes")[0] == 40, desc="and narrows")
        assert_eq(_h(tf, "content_widths"), before, "in either direction")          # 41
        # Squeezed to the floor, the text still reports its own width — which is
        # why measuring the painted cell answers "what does this need", not
        # "what did it get".
        v0 = _visual_of(tf, 0)
        squeezed = _rect(tf, f"colbody#0_{v0}")["w"]
        assert squeezed + 2 * CELL_PAD > 40, (
            f"the cell needs more than the 40px column it is in ({squeezed})"
        )                                                                            # 42
        # And switching back to contents gives it exactly what it needs again.
        tf.invoke("/external/set_resize_mode", "0:resize_to_contents")
        wait_until(lambda: _h(tf, "section_size.0") == before[0],
                   desc="the column fits its content again")                        # 43


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1453 §5.36 §2#7 — QFontMetrics parity: a view fn measures its own text",
        body,
    ))
